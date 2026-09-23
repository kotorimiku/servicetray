use std::{
    collections::HashMap,
    fs::OpenOptions,
    io,
    process::{Child, Command, Stdio},
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

    pub fn start(&self, config: &ProgramConfig, global_working_dir: &str) -> io::Result<()> {
        let mut processes = self.processes.lock().unwrap();

        if processes.contains_key(&config.name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                t!("already.running", name = config.name.clone()),
            ));
        }

        let exe_path = expand_tilde(&config.path);
        let mut cmd = Command::new(&exe_path);
        cmd.current_dir(expand_tilde(
            config.effective_working_dir(global_working_dir),
        ));
        if let Some(args) = &config.args {
            for arg in args {
                cmd.arg(expand_tilde_arg(arg));
            }
        }

        let log_path = config.log_path();
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let out_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)?;
        let err_file = out_file.try_clone()?;
        cmd.stdout(Stdio::from(out_file));
        cmd.stderr(Stdio::from(err_file));
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

    static PROCESS_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_process_manager_start_with_tilde_path() {
        let _lock = PROCESS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
            working_dir: None,
        };

        let start_res = manager.start(&config, "~");
        let _ = manager.stop("tilde_test_prog");
        let _ = fs::remove_file(&test_file);

        assert!(
            start_res.is_ok(),
            "Failed to start program with tilde path: {:?}",
            start_res
        );
    }

    #[test]
    fn test_process_manager_start_with_working_dir() {
        let _lock = PROCESS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let manager = ProcessManager::new();

        #[cfg(windows)]
        let shell_cmd = "cmd.exe";
        #[cfg(not(windows))]
        let shell_cmd = "sh";

        let non_existent_dir = std::env::temp_dir()
            .join("servicetray_non_existent_dir_999999")
            .to_string_lossy()
            .into_owned();

        // 1. Program without working_dir uses global working dir ("~")
        let config_default = ProgramConfig {
            name: "cwd_test_default".to_string(),
            path: shell_cmd.to_string(),
            args: None,
            service_url: None,
            watch_paths: None,
            working_dir: None,
        };
        let res = manager.start(&config_default, "~");
        assert!(res.is_ok(), "Failed to start with ~: {:?}", res);
        let _ = manager.stop("cwd_test_default");

        // 2. Program with non-existent working_dir fails to spawn
        let config_invalid = ProgramConfig {
            name: "cwd_test_invalid".to_string(),
            path: shell_cmd.to_string(),
            args: None,
            service_url: None,
            watch_paths: None,
            working_dir: Some(non_existent_dir.clone()),
        };
        let res_invalid = manager.start(&config_invalid, "~");
        assert!(
            res_invalid.is_err(),
            "Expected error for invalid working dir"
        );

        // 3. Program with non-existent global working_dir fails to spawn when program working_dir is None
        let config_inherit_invalid = ProgramConfig {
            name: "cwd_test_inherit_invalid".to_string(),
            path: shell_cmd.to_string(),
            args: None,
            service_url: None,
            watch_paths: None,
            working_dir: None,
        };
        let res_inherit_invalid = manager.start(&config_inherit_invalid, &non_existent_dir);
        assert!(
            res_inherit_invalid.is_err(),
            "Expected error when inheriting non-existent global working dir"
        );

        // 4. Valid per-program working_dir overrides invalid global working_dir
        let config_override = ProgramConfig {
            name: "cwd_test_override".to_string(),
            path: shell_cmd.to_string(),
            args: None,
            service_url: None,
            watch_paths: None,
            working_dir: Some("~".to_string()),
        };
        let res_override = manager.start(&config_override, &non_existent_dir);
        assert!(
            res_override.is_ok(),
            "Expected program working_dir to override invalid global: {:?}",
            res_override
        );
        let _ = manager.stop("cwd_test_override");
    }

    #[test]
    fn test_process_manager_start_with_log_file() {
        let _lock = PROCESS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let manager = ProcessManager::new();

        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let prog_name = format!("log_test_{unique_id}");

        #[cfg(windows)]
        let (cmd, args) = (
            "cmd.exe",
            vec!["/C".to_string(), "echo log_output_test".to_string()],
        );
        #[cfg(not(windows))]
        let (cmd, args) = (
            "sh",
            vec!["-c".to_string(), "echo log_output_test".to_string()],
        );

        let config = ProgramConfig {
            name: prog_name.clone(),
            path: cmd.to_string(),
            args: Some(args),
            service_url: None,
            watch_paths: None,
            working_dir: None,
        };

        let log_file = config.log_path();
        if let Some(parent) = log_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(&log_file, "previous_log_content\n").unwrap();

        let res = manager.start(&config, "~");
        assert!(res.is_ok(), "Failed to start program: {:?}", res);

        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = manager.stop(&prog_name);

        let content = fs::read_to_string(&log_file).unwrap_or_default();
        let _ = fs::remove_file(&log_file);
        assert!(
            content.contains("log_output_test"),
            "Expected log content, got: {content}"
        );
        assert!(
            !content.contains("previous_log_content"),
            "Expected previous log content to be overwritten, got: {content}"
        );
    }
}
