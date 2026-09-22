use std::{
    collections::HashMap,
    io,
    process::{Child, Command},
    sync::{Arc, Mutex},
};

use rust_i18n::t;
use tracing::info;

use crate::config::{ProgramConfig, expand_tilde, expand_tilde_arg};

fn kill_child(child: &mut Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &child.id().to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub struct ProcessManager {
    processes: Arc<Mutex<HashMap<String, Child>>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        ProcessManager {
            processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start(&self, config: &ProgramConfig) -> io::Result<()> {
        let mut processes = self.processes.lock().unwrap();

        if processes.contains_key(&config.name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                t!("already.running", name = config.name.clone()),
            ));
        }

        let exe_path = expand_tilde(&config.path);
        let mut cmd = Command::new(&exe_path);

        if let Some(args) = &config.args {
            for arg in args {
                cmd.arg(expand_tilde_arg(arg));
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let child = cmd.spawn()?;

        #[cfg(windows)]
        {
            assign_to_job_object(child.id());
        }

        processes.insert(config.name.clone(), child);

        info!("{}", t!("started.program", name = config.name.clone()));
        Ok(())
    }

    pub fn stop(&self, name: &str) -> io::Result<()> {
        let mut processes = self.processes.lock().unwrap();

        if let Some(mut child) = processes.remove(name) {
            kill_child(&mut child);
            info!("{}", t!("stopped.program", name = name.to_string()));
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                t!("not.running", name = name.to_string()),
            ));
        }

        Ok(())
    }

    pub fn stop_all(&self) {
        let mut processes = self.processes.lock().unwrap();

        for (name, mut child) in processes.drain() {
            kill_child(&mut child);
            info!("{}", t!("program.stopped", name = name));
        }
    }

    pub fn is_running(&self, name: &str) -> bool {
        let processes = self.processes.lock().unwrap();
        processes.contains_key(name)
    }

    pub fn running_processes(&self) -> Vec<String> {
        let processes = self.processes.lock().unwrap();
        processes.keys().cloned().collect()
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        info!("{}", t!("processmanager.destroy"));
        self.stop_all();

        // Windows: close Job Object handle to ensure all child processes are terminated
        #[cfg(windows)]
        {
            close_job_object();
        }
    }
}

#[cfg(windows)]
mod windows_job_object {
    use std::sync::Mutex;

    use windows::{
        Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::{
                JobObjects::{
                    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                    SetInformationJobObject,
                },
                Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
            },
        },
        core::PCWSTR,
    };

    static JOB_HANDLE: Mutex<usize> = Mutex::new(0);

    fn get_or_create() -> HANDLE {
        let mut guard = JOB_HANDLE.lock().unwrap();
        if *guard != 0 {
            return HANDLE(*guard as _);
        }

        unsafe {
            let job = match CreateJobObjectW(None, PCWSTR::null()) {
                Ok(h) => h,
                Err(_) => return HANDLE::default(),
            };

            if job.is_invalid() {
                return HANDLE::default();
            }

            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation:
                    windows::Win32::System::JobObjects::JOBOBJECT_BASIC_LIMIT_INFORMATION {
                        LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                        ..Default::default()
                    },
                ..Default::default()
            };

            let _ = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &mut info as *mut _ as *mut _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );

            *guard = job.0 as usize;
            job
        }
    }

    pub fn assign_process(pid: u32) {
        unsafe {
            let job = get_or_create();
            if job.is_invalid() {
                return;
            }

            if let Ok(process) = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) {
                let _ = AssignProcessToJobObject(job, process);
                let _ = CloseHandle(process);
            }
        }
    }

    pub fn close_job() {
        let mut guard = JOB_HANDLE.lock().unwrap();
        if *guard != 0 {
            unsafe {
                let _ = CloseHandle(HANDLE(*guard as _));
            }
            *guard = 0;
        }
    }
}

#[cfg(windows)]
use windows_job_object::{assign_process as assign_to_job_object, close_job as close_job_object};

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn test_process_manager_start_with_tilde_path() {
        let Some(home) = std::env::home_dir() else {
            return;
        };
        #[cfg(windows)]
        let test_file = home.join("servicetray_tilde_test.cmd");
        #[cfg(not(windows))]
        let test_file = home.join("servicetray_tilde_test.sh");

        #[cfg(windows)]
        fs::write(&test_file, "@echo hello\r\n").unwrap();
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::write(&test_file, "#!/bin/sh\necho hello\n").unwrap();
            let mut perms = fs::metadata(&test_file).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&test_file, perms).unwrap();
        }

        #[cfg(windows)]
        let tilde_path = "~/servicetray_tilde_test.cmd";
        #[cfg(not(windows))]
        let tilde_path = "~/servicetray_tilde_test.sh";

        let manager = ProcessManager::new();
        let config = ProgramConfig {
            name: "tilde_test_prog".to_string(),
            path: tilde_path.to_string(),
            args: Some(vec!["~/dummy_arg".to_string()]),
            service_url: None,
            watch_paths: None,
        };

        let start_res = manager.start(&config);
        let _ = manager.stop("tilde_test_prog");
        let _ = fs::remove_file(&test_file);

        assert!(
            start_res.is_ok(),
            "Failed to start program with tilde path: {:?}",
            start_res
        );
    }
}
