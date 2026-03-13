use std::{io::Sink, path::Path};

use color_eyre::eyre::Result;
use tracing::metadata::LevelFilter;

pub fn init(
    log_level: Option<&str>,
    save_log_file: bool,
) -> Result<tracing_appender::non_blocking::WorkerGuard> {
    let log_level = log_level
        .and_then(|lvl| lvl.parse::<LevelFilter>().ok())
        .unwrap_or(LevelFilter::INFO);

    if save_log_file {
        let log_dir = Path::new("logs");
        std::fs::create_dir_all(log_dir)?;

        // rolling appender (simple and reliable cross-platform)
        let file_appender = tracing_appender::rolling::never("logs", "servicetray.log");

        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        tracing_subscriber::fmt()
            .with_max_level(log_level)
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(non_blocking)
            .with_ansi(false)
            .init();

        Ok(guard)
    } else {
        // Only output to stdout, no file writing
        tracing_subscriber::fmt()
            .with_max_level(log_level)
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();

        // Create a dummy guard with a sink that discards all output
        let (_, guard) = tracing_appender::non_blocking(Sink::default());
        Ok(guard)
    }
}
