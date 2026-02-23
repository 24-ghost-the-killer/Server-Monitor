use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use tower_http::services::ServeDir;
use std::net::SocketAddr;
use tracing::info;

use crate::models::CheckResult;
use crate::engine::Monitor;

pub async fn get_stats(
    State(monitor): State<Arc<Monitor>>
) -> Json<Vec<CheckResult>> {
    let mut results: Vec<CheckResult> = if let Some(redis) = &monitor.redis {
        match redis.fetch_all_results().await {
            Ok(data) => data.values().cloned().collect(),
            Err(_) => {
                let state = monitor.state.lock().await;
                state.last_results.values().cloned().collect()
            }
        }
    } else {
        let state = monitor.state.lock().await;
        state.last_results.values().cloned().collect()
    };
    
    if monitor.config.hide_endpoints {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        for res in &mut results {
            let mut hasher = DefaultHasher::new();
            res.target_address.hash(&mut hasher);
            res.target_address = format!("MASKED-{}", hasher.finish());

            let mut p_hasher = DefaultHasher::new();
            res.parent_address.hash(&mut p_hasher);
            res.parent_address = format!("MASKED-{}", p_hasher.finish());
        }
    }
    
    Json(results)
}

pub fn create_router(monitor: Arc<Monitor>) -> Router {
    Router::new()
        .route("/api/stats", get(get_stats))
        .fallback_service(ServeDir::new("public"))
        .with_state(monitor)
}

pub async fn start_server(monitor: Arc<Monitor>) {
    let port = monitor.config.api_port;
    let app = create_router(monitor);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Dashboard: http://localhost:{}", addr.port());
    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind API port");
    axum::serve(listener, app).await.unwrap();
}
