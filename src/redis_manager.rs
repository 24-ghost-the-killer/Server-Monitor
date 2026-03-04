use redis::{AsyncCommands, Client};
use anyhow::{Result, Context};
use crate::models::CheckResult;
use std::collections::HashMap;
use tracing::warn;


#[derive(Clone)]
pub struct RedisManager {
    client: Client,
    prefix: String,
}

impl RedisManager {
    pub fn new(url: &str, prefix: String) -> Result<Self> {
        let client = Client::open(url).context("Failed to open Redis client")?;
        Ok(Self { client, prefix })
    }

    pub async fn register_node(&self, node_id: &str) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = format!("{}:node:{}", self.prefix, node_id);
        let set_key = format!("{}:nodes", self.prefix);
        
        let _: () = conn.set_ex(&key, "online", 5).await?;
        let _: () = conn.sadd(&set_key, node_id).await?;
        Ok(())
    }

    pub async fn unregister_node(&self, node_id: &str) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = format!("{}:node:{}", self.prefix, node_id);
        let set_key = format!("{}:nodes", self.prefix);
        
        let _: () = conn.del(&key).await?;
        let _: () = conn.srem(&set_key, node_id).await?;
        Ok(())
    }

    pub async fn cleanup_dead_nodes(&self) -> Result<Vec<String>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let set_key = format!("{}:nodes", self.prefix);
        let nodes: Vec<String> = conn.smembers(&set_key).await?;
        
        let mut live_nodes = Vec::new();
        for node in nodes {
            let key = format!("{}:node:{}", self.prefix, node);
            let exists: bool = conn.exists(&key).await?;
            if exists {
                live_nodes.push(node);
            } else {
                warn!("Node {} has left the cluster (or timed out). Purging from mesh...", node);
                let _: () = conn.srem(&set_key, &node).await?;
            }
        }
        live_nodes.sort();
        Ok(live_nodes)
    }

    pub async fn push_result(&self, key: &str, result: &CheckResult) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let results_key = format!("{}:results", self.prefix);
        let json = serde_json::to_string(result)?;
        let _: () = conn.hset(&results_key, key, json).await?;
        Ok(())
    }

    pub async fn fetch_all_results(&self) -> Result<HashMap<String, CheckResult>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let results_key = format!("{}:results", self.prefix);
        let data: HashMap<String, String> = conn.hgetall(&results_key).await?;
        
        let mut results = HashMap::new();
        for (key, json) in data {
            if let Ok(res) = serde_json::from_str(&json) {
                results.insert(key, res);
            }
        }
        Ok(results)
    }

    pub async fn delete_result(&self, key: &str) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let results_key = format!("{}:results", self.prefix);
        let _: () = conn.hdel(&results_key, key).await?;
        Ok(())
    }

    pub async fn try_acquire_lock(&self, resource: &str, node_id: &str, ttl_ms: u64) -> bool {
        let mut conn = match self.client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(_) => return false,
        };
        let lock_key = format!("{}:lock:{}", self.prefix, resource);
        let res: Option<String> = redis::Cmd::set(&lock_key, node_id)
            .arg("NX")
            .arg("PX")
            .arg(ttl_ms)
            .query_async(&mut conn)
            .await
            .ok();
        
        res.is_some()
        }

    pub async fn is_rate_limited(&self, max_per_second: u64) -> Result<bool> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let now = chrono::Utc::now().timestamp();
        let key = format!("{}:ratelimit:{}", self.prefix, now);
        
        let count: u64 = conn.incr(&key, 1).await?;
        if count == 1 {
            let _: () = conn.expire(&key, 2).await?;
        }
        
        Ok(count > max_per_second)
    }

}