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
use crate::config::MonitorConfig;

pub async fn get_stats(
    State((state, config)): State<(Arc<Mutex<MonitorState>>, Arc<MonitorConfig>)>
) -> Json<Vec<CheckResult>> {
    let state = state.lock().await;
    let mut results: Vec<CheckResult> = state.last_results.values().cloned().collect();
    
    if config.hide_endpoints {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        for res in &mut results {
            // Uniquely mask target_address
            let mut hasher = DefaultHasher::new();
            res.target_address.hash(&mut hasher);
            res.target_address = format!("MASKED-{}", hasher.finish());

            // Uniquely mask parent_address
            let mut p_hasher = DefaultHasher::new();
            res.parent_address.hash(&mut p_hasher);
            res.parent_address = format!("MASKED-{}", p_hasher.finish());
        }
    }
    
    Json(results)
}

pub fn create_router(state: Arc<Mutex<MonitorState>>, config: MonitorConfig) -> Router {
    let shared_config = Arc::new(config);
    Router::new()
        .route("/api/stats", get(get_stats))
        .fallback_service(ServeDir::new("public"))
        .with_state((state, shared_config))
}

pub async fn start_server(config: MonitorConfig, state: Arc<Mutex<MonitorState>>) {
    let port = config.api_port;
    let app = create_router(state, config);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Dashboard: http://localhost:{}", addr.port());
    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind API port");
    axum::serve(listener, app).await.unwrap();
}
