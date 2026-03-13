use clap::Parser;

#[derive(Parser, Clone)]
#[command(version = "0.1", about)]
pub struct Args {
    #[arg(short, long, help = "Set the log level")]
    pub log_level: Option<String>,
    #[arg(long, help = "Save log to file")]
    pub save_log_file: Option<bool>,
}
