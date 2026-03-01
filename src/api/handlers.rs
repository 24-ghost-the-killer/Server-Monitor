use axum::{
    extract::State,
    Json,
};
use std::sync::Arc;
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
    
    let hide = monitor.config.read().await.hide_endpoints;
    if hide {
        for res in &mut results {
            res.mask_addresses();
        }
    }
    
    Json(results)
}
