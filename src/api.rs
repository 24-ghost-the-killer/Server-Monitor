use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::services::ServeDir;
use std::net::SocketAddr;
use tracing::info;

use crate::models::{CheckResult, MonitorState};

pub async fn get_stats(State(state): State<Arc<Mutex<MonitorState>>>) -> Json<Vec<CheckResult>> {
    let state = state.lock().await;
    Json(state.last_results.values().cloned().collect())
}

pub fn create_router(state: Arc<Mutex<MonitorState>>) -> Router {
    Router::new()
        .route("/api/stats", get(get_stats))
        .fallback_service(ServeDir::new("public"))
        .with_state(state)
}

pub async fn start_server(port: u16, state: Arc<Mutex<MonitorState>>) {
    let app = create_router(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Dashboard: http://localhost:{}", addr.port());
    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind API port");
    axum::serve(listener, app).await.unwrap();
}
