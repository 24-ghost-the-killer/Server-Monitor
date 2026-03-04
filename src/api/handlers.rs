use axum::{
    extract::State,
    Json,
};
use std::sync::Arc;
use crate::models::{CheckResult, StatsResponse, CategoryStats, ServerStats, Status};
use crate::engine::Monitor;
use std::collections::HashMap;
use chrono::Utc;

pub async fn get_stats(
    State(monitor): State<Arc<Monitor>>
) -> Json<StatsResponse> {
    let results: Vec<CheckResult> = if let Some(redis) = &monitor.redis {
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
    let node_id = monitor.state.lock().await.node_id.clone();
    
    let mut cat_map: HashMap<String, HashMap<String, Vec<CheckResult>>> = HashMap::new();
    let mut server_addresses: HashMap<String, String> = HashMap::new();

    for mut res in results {
        if hide {
            res.mask_addresses();
        }
        server_addresses.insert(res.server_name.clone(), res.parent_address.clone());
        cat_map.entry(res.category.clone())
            .or_insert_with(HashMap::new)
            .entry(res.server_name.clone())
            .or_insert_with(Vec::new)
            .push(res);
    }

    let mut categories = Vec::new();
    for (cat_name, servers_map) in cat_map {
        let mut servers = Vec::new();
        for (srv_name, mut checks) in servers_map {
            checks.sort_by_key(|c| c.check_order);
            let srv_status = if checks.iter().any(|c| c.status) { Status::Up } else { Status::Down };
            let address = server_addresses.get(&srv_name).cloned().unwrap_or_default();
            
            servers.push(ServerStats {
                name: srv_name,
                address,
                status: srv_status,
                checks,
            });
        }
        servers.sort_by_key(|s| s.checks.first().map(|c| c.server_order).unwrap_or(0));
        categories.push(CategoryStats {
            name: cat_name,
            servers,
        });
    }
    categories.sort_by_key(|c| c.servers.first().and_then(|s| s.checks.first()).map(|ch| ch.category_order).unwrap_or(0));

    Json(StatsResponse {
        node_id,
        timestamp: Utc::now(),
        categories,
    })
}

