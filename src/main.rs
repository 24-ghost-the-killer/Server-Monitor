use anyhow::{Result, Context};
use std::sync::Arc;
use tokio::signal;
use tracing::info;

mod config;
mod models;
mod engine;
mod api;
mod utils;

use crate::config::MonitorConfig;
use crate::engine::Monitor;

#[tokio::main]
async fn main() -> Result<()> {
    utils::setup_console();
    
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive(tracing::Level::INFO.into()))
        .with_ansi(true)
        .init();

    let config_path = "config.json";
    let config_content = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read {}", config_path))?;
    let config: MonitorConfig = serde_json::from_str(&config_content)
        .with_context(|| "Failed to parse config")?;

    let monitor = Arc::new(Monitor::new(config.clone()).await?);
    let state_for_api = monitor.state.clone();
    let api_port = config.api_port;

    tokio::spawn(async move {
        api::start_server(api_port, state_for_api).await;
    });

    let monitor_clone = Arc::clone(&monitor);
    tokio::spawn(async move {
        if let Err(e) = monitor_clone.run().await {
            tracing::error!("Monitor engine failed: {}", e);
        }
    });

    signal::ctrl_c().await?;
    info!("Shutdown signal received. Closing NetPulse Engine...");
    
    Ok(())
}
