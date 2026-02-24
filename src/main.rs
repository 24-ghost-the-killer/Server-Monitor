use anyhow::{Result, Context};
use std::sync::Arc;
use tokio::signal;
use tracing::info;
use clap::Parser;

mod config;
mod models;
mod engine;
mod api;
mod utils;
mod redis_manager;

use crate::config::MonitorConfig;
use crate::engine::Monitor;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "config.json")]
    config: String,

    #[arg(long, env = "API_PORT")]
    api_port: Option<u16>,

    #[arg(long, env = "NODE_ID")]
    node_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    utils::setup_console();
    
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive(tracing::Level::INFO.into()))
        .with_ansi(true)
        .init();

    let args = Args::parse();
    let config_path = &args.config;
    
    let local_config_content = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read baseline config {}", config_path))?;
    let mut config: MonitorConfig = serde_json::from_str(&local_config_content)
        .with_context(|| "Failed to parse baseline config")?;

    if let Some(p) = args.api_port { config.api_port = p; }

    let monitor = Arc::new(Monitor::new(config.clone()).await?);
    
    if let Some(nid) = args.node_id {
        let mut state = monitor.state.lock().await;
        state.node_id = nid;
    }

    let should_run_dashboard = match config.enable_dashboard {
        Some(enabled) => enabled,
        None => config.redis_url.is_none(),
    };

    if should_run_dashboard {
        let monitor_for_api = monitor.clone();
        tokio::spawn(async move {
            api::start_server(monitor_for_api).await;
        });
    } else {
        info!("Mesh Cluster Context: Dashboard disabled on this node.");
    }

    let monitor_clone = Arc::clone(&monitor);
    tokio::spawn(async move {
        if let Err(e) = monitor_clone.run().await {
            tracing::error!("Monitor engine failed: {}", e);
        }
    });

    signal::ctrl_c().await?;
    info!("Shutdown signal received. Closing NetPulse Engine...");
    monitor.shutdown().await;
    
    Ok(())
}
