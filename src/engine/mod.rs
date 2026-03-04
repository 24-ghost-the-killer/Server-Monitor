use anyhow::{Result, Context};
use chrono::Utc;
use futures::stream::{FuturesUnordered, StreamExt};
use hickory_resolver::config::ResolverConfig;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::TokioResolver;
use ipnet::IpNet;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use surge_ping::{Client as PingClient, Config as PingConfig};
use tokio::sync::{Mutex, Semaphore, RwLock};
use tracing::{error, info, warn};

use crate::config::{MonitorConfig, CheckType};
use crate::models::{CheckResult, MonitorState, Status};
use crate::redis_manager::RedisManager;

pub mod checks;
pub mod notifications;

pub struct Monitor {
    pub config: Arc<RwLock<MonitorConfig>>,
    pub(crate) ping_client: PingClient,
    pub state: Arc<Mutex<MonitorState>>,
    pub(crate) http_client: reqwest::Client,
    pub(crate) concurrency_limiter: Arc<Semaphore>,
    pub(crate) dns_resolver: TokioResolver,
    pub redis: Option<RedisManager>,
}


impl Monitor {
    pub async fn new(config: MonitorConfig) -> Result<Self> {
        let ping_client = PingClient::new(&PingConfig::default())
            .context("Failed to create Ping Client")?;
        
        let max_concurrent = config.max_concurrency;

        let dns_resolver = TokioResolver::builder_with_config(
            ResolverConfig::cloudflare(),
            TokioConnectionProvider::default(),
        ).build();

        info!("DNS resolver configured: Cloudflare 1.1.1.1 / 1.0.0.1");

        let redis = if let Some(url) = &config.redis_url {
            info!("Redis clustering mode enabled: {}", url);
            Some(RedisManager::new(url, config.redis_prefix.clone())?)
        } else {
            None
        };

        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string());
        let node_id = format!("{}-{}", hostname, &uuid::Uuid::new_v4().to_string()[..4]);

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            ping_client,
            state: Arc::new(Mutex::new(MonitorState {
                last_results: HashMap::new(),
                node_id,
                live_nodes: Vec::new(),
                last_loss_alerts: HashMap::new(),
            })),
            http_client: reqwest::Client::new(),
            concurrency_limiter: Arc::new(Semaphore::new(max_concurrent)),
            dns_resolver,
            redis,
        })
    }


    pub async fn initialize_state(&self) {
        info!("Initializing infrastructure mesh state...");
        
        let mut active_keys = std::collections::HashSet::new();
        let now = Utc::now();

        let cfg = self.config.read().await;
        for category in cfg.categories.iter() {
            for server in category.servers.iter() {
                let addresses = if let Ok(net) = server.address.parse::<IpNet>() {
                    net.hosts().map(|ip| ip.to_string()).collect::<Vec<_>>()
                } else {
                    vec![server.address.clone()]
                };

                for address in addresses {
                    for check in server.checks.iter() {
                        let check_type_name = match check {
                            CheckType::Ping { .. } => "ICMP".into(),
                            CheckType::TcpPort { port, .. } => format!("TCP:{}", port),
                            CheckType::UdpPort { port, .. } => format!("UDP:{}", port),
                            CheckType::Http { method, .. } => format!("HTTP:{}", method.as_deref().unwrap_or("GET")),
                        };
                        let key = format!("{}-{}-{}-{}", server.name, server.address, address, check_type_name);
                        active_keys.insert(key);
                    }
                }
            }
        }

        if let Some(redis) = &self.redis {
            match redis.fetch_all_results().await {
                Ok(cached) => {
                    let mut state = self.state.lock().await;
                    
                    for key in cached.keys() {
                        if !active_keys.contains(key) {
                            let _ = redis.delete_result(key).await;
                        }
                    }

                    state.last_results = cached.into_iter()
                        .filter(|(k, _)| active_keys.contains(k))
                        .collect();

                    info!("Restored {} valid tracking points from Redis cache", state.last_results.len());
                }
                Err(e) => error!("Failed to fetch mesh state from Redis: {}", e),
            }
        }

        let mut state = self.state.lock().await;
        let cfg = self.config.read().await;
        for (cat_idx, category) in cfg.categories.iter().enumerate() {
            for (srv_idx, server) in category.servers.iter().enumerate() {
                let addresses = if let Ok(net) = server.address.parse::<IpNet>() {
                    net.hosts().map(|ip| ip.to_string()).collect::<Vec<_>>()
                } else {
                    vec![server.address.clone()]
                };

                for address in addresses {
                    for (chk_idx, check) in server.checks.iter().enumerate() {
                        let check_type_name = match check {
                            CheckType::Ping { .. } => "ICMP".into(),
                            CheckType::TcpPort { port, .. } => format!("TCP:{}", port),
                            CheckType::UdpPort { port, .. } => format!("UDP:{}", port),
                            CheckType::Http { method, .. } => format!("HTTP:{}", method.as_deref().unwrap_or("GET")),
                        };
                        let key = format!("{}-{}-{}-{}", server.name, server.address, address, check_type_name);
                        
                        state.last_results.entry(key).or_insert(CheckResult {
                            category: category.name.clone(),
                            server_name: server.name.clone(),
                            parent_address: server.address.clone(),
                            target_address: address.clone(),
                            timestamp: now,
                            check_type: check_type_name,
                            status: false,
                            latency_ms: None,
                            packet_loss: None,
                            message: "Awaiting Infrastructure Handshake...".into(),
                            category_order: cat_idx,
                            server_order: srv_idx,
                            check_order: chk_idx,
                            provider_node: None,
                        });
                    }
                }
            }
        }
        info!("Mesh state warmed with {} tracking points", state.last_results.len());
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        info!("Engine V4: Turbo Mesh Monitoring Active...");
        {
            let cfg = self.config.read().await;
            info!("--- Max Concurrency: {} workers ---", cfg.max_concurrency);
        }
        
        self.initialize_state().await;
        
        let node_id = {
            let state = self.state.lock().await;
            state.node_id.clone()
        };
        
        if let Some(redis) = &self.redis {
            let redis_clone = redis.clone();
            let state_clone = Arc::clone(&self.state);
            let nid = node_id.clone();
            
            tokio::spawn(async move {
                let mut is_online = true;
                let mut last_nodes: Vec<String> = Vec::new();

                loop {
                    let _ = redis_clone.register_node(&nid).await;
                    match redis_clone.cleanup_dead_nodes().await {
                        Ok(current_nodes) => {
                            let mut state = state_clone.lock().await;
                            state.live_nodes = current_nodes.clone();
                            
                            if !is_online {
                                warn!("--- Redis Cluster Mesh Link Restored ---");
                                is_online = true;
                            }

                            for node in &current_nodes {
                                if !last_nodes.contains(node) && node != &nid {
                                    info!("Cluster Event: Node [{}] joined the infrastructure mesh.", node);
                                }
                            }

                            for node in &last_nodes {
                                if !current_nodes.contains(node) && node != &nid {
                                    warn!("Cluster Event: Node [{}] has disconnected or timed out.", node);
                                }
                            }

                            if current_nodes.len() == 1 && last_nodes.len() > 1 {
                                info!("Cluster Status: Last peer disconnected. Resuming full local workload (Standalone).");
                            } else if current_nodes.len() > 1 && last_nodes.len() <= 1 {
                                info!("Cluster Status: Peer(s) detected. Load-balancing turbo mesh active.");
                            } else if current_nodes.len() != last_nodes.len() && current_nodes.len() > 1 {
                                info!("Cluster Status: Mesh updated. Total nodes active: {}.", current_nodes.len());
                            }

                            last_nodes = current_nodes;
                        }
                        Err(e) => {
                            if is_online {
                                error!("Redis cluster heartbeat failure: {}. Mesh coordination suspended.", e);
                                is_online = false;
                                last_nodes.clear();
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });
        }

        let mut first_run = true;
        loop {
            let interval = self.config.read().await.check_interval;
            let now_ms = Utc::now().timestamp_millis() as u64;
            let interval_ms = interval * 1000;
            let next_tick_ms = ((now_ms / interval_ms) + 1) * interval_ms;
            let sleep_ms = next_tick_ms - now_ms;
            
            if sleep_ms > 40 && !first_run {
                tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
            }
            first_run = false;

            let start_time = Utc::now();
            let mut tasks = FuturesUnordered::new();

            let live_nodes = if self.redis.is_some() {
                let state = self.state.lock().await;
                if state.live_nodes.is_empty() {
                    vec![node_id.clone()]
                } else {
                    state.live_nodes.clone()
                }
            } else {
                vec![node_id.clone()]
            };

            let node_index = live_nodes.iter().position(|id| id == &node_id).unwrap_or(0);
            let total_nodes = live_nodes.len();

            if total_nodes > 1 {
                info!("Mesh Sync: Handshaking with cluster... [Local Node {}/{}]", node_index + 1, total_nodes);
            }

            let mut skipped = 0;
            let mut performed = 0;
            let mut global_idx = 0;

            let cfg = self.config.read().await;
            for (cat_idx, category) in cfg.categories.iter().enumerate() {
                for (srv_idx, server) in category.servers.iter().enumerate() {

                    let addresses = if let Ok(net) = server.address.parse::<IpNet>() {
                        net.hosts().map(|ip| ip.to_string()).collect::<Vec<_>>()
                    } else {
                        vec![server.address.clone()]
                    };

                    for address in addresses {
                        for (chk_idx, check) in server.checks.iter().enumerate() {
                            let check_type_name = match check {
                                CheckType::Ping { .. } => "ICMP".into(),
                                CheckType::TcpPort { port, .. } => format!("TCP:{}", port),
                                CheckType::UdpPort { port, .. } => format!("UDP:{}", port),
                                CheckType::Http { method, .. } => format!("HTTP:{}", method.as_deref().unwrap_or("GET")),
                            };
                            
                            let key = format!("{}-{}-{}-{}", server.name, server.address, address, check_type_name);
                            
                            let item_idx = global_idx;
                            global_idx += 1;
                            
                            if (item_idx % total_nodes) != node_index {
                                skipped += 1;
                                continue;
                            }

                            performed += 1;

                            let monitor_ref = Arc::clone(&self);
                            let s_clone = server.clone();
                            let a_clone = address.clone();
                            let c_clone = check.clone();
                            let cat_name = category.name.clone();
                            let nid_clone = node_id.clone();
                            let key_clone = key.clone();
                            let interval = cfg.check_interval;
                            let max_rps = cfg.max_checks_per_second;
                            
                            tasks.push(tokio::spawn(async move {
                                if let Some(redis) = &monitor_ref.redis {
                                    if redis.is_rate_limited(max_rps).await.unwrap_or(false) {
                                        return None;
                                    }

                                    let ttl = (interval * 1000) * 8 / 10;
                                    if !redis.try_acquire_lock(&key_clone, &nid_clone, ttl).await {
                                        return None;
                                    }
                                }

                                let _permit = monitor_ref.concurrency_limiter.acquire().await.ok();
                                let mut res = monitor_ref.run_check_with_retry(s_clone, a_clone, c_clone).await;
                                res.category = cat_name;
                                res.category_order = cat_idx;
                                res.server_order = srv_idx;
                                res.check_order = chk_idx;
                                res.provider_node = Some(nid_clone);
                                Some(res)
                            }));

                            tokio::time::sleep(Duration::from_millis(15)).await;
                        }
                    }
                }
            }

            let mut results = Vec::with_capacity(tasks.len());
            let total = tasks.len();
            while let Some(join_res) = tasks.next().await {
                if let Ok(Some(result)) = join_res {
                    results.push(result);
                }
            }

            results.sort_by(|a, b| {
                a.category_order.cmp(&b.category_order)
                    .then(a.server_order.cmp(&b.server_order))
                    .then(a.server_name.cmp(&b.server_name))
                    .then(a.check_type.cmp(&b.check_type))
            });

            for result in results {
                self.process_result(result).await;
            }

            let duration = Utc::now() - start_time;
            if total > 0 {
                info!("Turbo cycle: {} performed, {} delegated to cluster. Finished in {:.2}s.", 
                    performed,
                    skipped,
                    duration.num_milliseconds() as f64 / 1000.0);
            }

        }
    }

    pub async fn process_result(self: &Arc<Self>, result: CheckResult) {
        let key = format!("{}-{}-{}-{}", result.server_name, result.parent_address, result.target_address, result.check_type);
        let new_status = if result.status { Status::Up } else { Status::Down };
        
        // Fetch config once at the start
        let cfg = self.config.read().await;
        
        if let Some(redis) = &self.redis {
            let _ = redis.push_result(&key, &result).await;
        }


        let mut state_lock = self.state.lock().await;
        let old_result = state_lock.last_results.get(&key).cloned();
        state_lock.last_results.insert(key.clone(), result.clone());

        let global_threshold = cfg.packet_loss_threshold;

        let threshold = {
            let mut t = global_threshold;
            for cat in &cfg.categories {
                if cat.name == result.category {
                    for srv in &cat.servers {
                        if srv.name == result.server_name {
                            if let Some(st) = srv.packet_loss_threshold {
                                t = st;
                            }
                            break;
                        }
                    }
                }
            }
            t
        };

        if let Some(loss) = result.packet_loss {
            let last_alert_loss = state_lock.last_loss_alerts.get(&key).cloned().unwrap_or(0.0);
            
            if loss >= threshold && last_alert_loss < threshold && result.status {
                state_lock.last_loss_alerts.insert(key.clone(), loss);
                let this = Arc::clone(self);
                let res_clone = result.clone();
                tokio::spawn(async move { this.dispatch_loss_notification(res_clone, loss, threshold, true).await; });
            } 

            else if loss < threshold && last_alert_loss >= threshold {
                state_lock.last_loss_alerts.remove(&key);
                let this = Arc::clone(self);
                let res_clone = result.clone();
                tokio::spawn(async move { this.dispatch_loss_notification(res_clone, loss, threshold, false).await; });
            }
        }
        drop(state_lock);
        
        let is_awaiting = old_result.as_ref().map_or(true, |r| r.message == "Awaiting Infrastructure Handshake...");
        let old_status = old_result.map(|r| if r.status { Status::Up } else { Status::Down });

        let old = match old_status {
            Some(old) if old != new_status => Some(old),
            _ if is_awaiting && new_status == Status::Down => Some(Status::Up),
            _ => None,
        };

        let old = match old {
            Some(o) => o,
            None => return,
        };

        let msg = format!("[CHANGE] {}/{} ({}) -> {:?}", result.server_name, result.check_type, result.target_address, new_status);
        if new_status == Status::Down { error!("{}", msg); } else { warn!("{}", msg); }

        if is_awaiting && new_status == Status::Up {
            return;
        }

        if cfg.webhook_url.is_some() || cfg.ntfy_topic.is_some() {
            let this = Arc::clone(self);
            tokio::spawn(async move { this.dispatch_notifications(result, old, new_status).await; });
        }
    }

    pub async fn shutdown(&self) {
        if let Some(redis) = &self.redis {
            let node_id = self.state.lock().await.node_id.clone();
            info!("Gracefully unregistering node {} from cluster...", node_id);
            let _ = redis.unregister_node(&node_id).await;
        }
    }
}
