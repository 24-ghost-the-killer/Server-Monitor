use axum::{
    routing::get,
    Router,
};
use std::sync::Arc;
use tower_http::services::ServeDir;
use std::net::SocketAddr;
use tracing::info;

use crate::engine::Monitor;

pub mod handlers;

pub fn create_router(monitor: Arc<Monitor>) -> Router {
    Router::new()
        .route("/api/stats", get(handlers::get_stats))
        .fallback_service(ServeDir::new("public"))
        .with_state(monitor)
}

pub async fn start_server(monitor: Arc<Monitor>) {
    let port = monitor.config.read().await.api_port;
    let app = create_router(monitor);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Dashboard: http://localhost:{}", addr.port());
    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind API port");
    axum::serve(listener, app).await.unwrap();
}
