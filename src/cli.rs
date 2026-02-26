use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "lnch")]
#[command(about = "A TUI multi-process launcher for your dev environment")]
#[command(version)]
pub struct Cli {
    /// Path to the config file
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}
