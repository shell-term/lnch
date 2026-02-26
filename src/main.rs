mod cli;
mod config;
mod error;
mod log;
mod message;
mod process;
mod tui;

use clap::Parser;
use tokio::sync::mpsc;

use crate::cli::Cli;
use crate::config::loader::{config_base_dir, find_config, load_config};
use crate::config::validator::validate_config;
use crate::message::{ProcessCommand, ProcessEvent};
use crate::process::dependency::DependencyGraph;
use crate::process::manager::ProcessManager;
use crate::process::signal::setup_signal_handler;
use crate::tui::app::App;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let config_path = match cli.config {
        Some(path) => path,
        None => find_config()?,
    };

    let config = load_config(&config_path)?;
    let base_dir = config_base_dir(&config_path);
    validate_config(&config, &base_dir).map_err(|e| anyhow::anyhow!("{}", e))?;

    let dependency_graph = DependencyGraph::from_config(&config)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let (cmd_tx, cmd_rx) = mpsc::channel::<ProcessCommand>(64);
    let (event_tx, event_rx) = mpsc::channel::<ProcessEvent>(256);

    // Set up signal handler for graceful shutdown
    let shutdown_cmd_tx = cmd_tx.clone();
    tokio::spawn(async move {
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        tokio::spawn(async move {
            setup_signal_handler(shutdown_tx).await;
        });
        if shutdown_rx.recv().await.is_some() {
            let _ = shutdown_cmd_tx.send(ProcessCommand::Shutdown).await;
        }
    });

    // Spawn ProcessManager
    let mut process_manager = ProcessManager::new(&config, dependency_graph, cmd_rx, event_tx);
    tokio::spawn(async move {
        process_manager.run().await;
    });

    // Run TUI application
    let app = App::new(&config, cmd_tx, event_rx);
    app.run().await?;

    Ok(())
}
