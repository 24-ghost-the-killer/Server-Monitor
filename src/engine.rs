use anyhow::{Result, Context};
use chrono::Utc;
use futures::stream::{FuturesUnordered, StreamExt};
use hickory_resolver::config::ResolverConfig;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::TokioResolver;
use ipnet::IpNet;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use surge_ping::{Client as PingClient, Config as PingConfig, PingIdentifier, PingSequence};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, Semaphore, RwLock};
use tracing::{error, info, warn};

use crate::config::{MonitorConfig, Server, CheckType};
use crate::models::{CheckResult, MonitorState, Status};
use crate::redis_manager::RedisManager;

pub struct Monitor {
    pub config: Arc<RwLock<MonitorConfig>>,
    ping_client: PingClient,
    pub state: Arc<Mutex<MonitorState>>,
    http_client: reqwest::Client,
    concurrency_limiter: Arc<Semaphore>,
    dns_resolver: TokioResolver,
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
                            CheckType::Ping { .. } => "Ping".into(),
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
                            CheckType::Ping { .. } => "Ping".into(),
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

        loop {
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
                                CheckType::Ping { .. } => "Ping".into(),
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

            let interval = self.config.read().await.check_interval;
            tokio::time::sleep(Duration::from_secs(interval)).await;
        }
    }

    async fn run_check_with_retry(&self, server: Server, target_address: String, check: CheckType) -> CheckResult {
        let mut last_result = self.perform_check(&server, &target_address, &check).await;
        
        if !last_result.status && server.max_retries > 0 {
            for _ in 1..=server.max_retries {
                tokio::time::sleep(Duration::from_millis(50)).await;
                last_result = self.perform_check(&server, &target_address, &check).await;
                if last_result.status {
                    break;
                }
            }
        }
        last_result
    }

    async fn perform_check(&self, server: &Server, target_address: &str, check: &CheckType) -> CheckResult {
        let timestamp = Utc::now();
        match check {
            CheckType::Ping { count, timeout_ms } => {
                let (mut status, mut latency, mut loss, mut msg) = self.check_ping(target_address, *count, *timeout_ms).await;
                
                if !status {
                    const DISCOVERY_PORTS: &[(u16, &str)] = &[
                        (22, "SSH"), (3389, "RDP"), (80, "HTTP"), (443, "HTTPS"),
                        (3306, "MySQL"), (30120, "FXServer"), (8080, "Web-Alt"),
                        (5900, "VNC"), (27015, "Source"), (25565, "Minecraft"),
                    ];

                    let mut discovery_tasks = FuturesUnordered::new();
                    for &(port, name) in DISCOVERY_PORTS {
                        let target = target_address.to_string();
                        discovery_tasks.push(async move {
                            let (p_status, p_latency, _) = Self::raw_tcp_check(&target, port, 1200).await;
                            (p_status, p_latency, name)
                        });
                    }

                    while let Some((p_status, p_latency, name)) = discovery_tasks.next().await {
                        if p_status {
                            status = true;
                            latency = p_latency; 
                            loss = Some(0.0);
                            msg = format!("ICMP Blocked (Handshake verified via {} - {}ms)", 
                                name, 
                                p_latency.map_or("N/A".to_string(), |l| format!("{:.1}", l)));
                            break;
                        }
                    }
                }

                CheckResult {
                    category: String::new(),
                    server_name: server.name.clone(),
                    parent_address: server.address.clone(),
                    target_address: target_address.to_string(),
                    timestamp,
                    check_type: "Ping".into(),
                    status,
                    latency_ms: latency,
                    packet_loss: loss,
                    message: msg,
                    category_order: 0,
                    server_order: 0,
                    check_order: 0,
                    provider_node: None,
                }
            }
            CheckType::TcpPort { port, count, timeout_ms } => {
                let (status, latency, loss, msg) = self.check_tcp_port(target_address, *port, *count, *timeout_ms).await;
                CheckResult {
                    category: String::new(),
                    server_name: server.name.clone(),
                    parent_address: server.address.clone(),
                    target_address: target_address.to_string(),
                    timestamp,
                    check_type: format!("TCP:{}", port),
                    status,
                    latency_ms: latency,
                    packet_loss: Some(loss),
                    message: msg,
                    category_order: 0,
                    server_order: 0,
                    check_order: 0,
                    provider_node: None,
                }
            }
            CheckType::UdpPort { port, count, timeout_ms } => {
                let (status, latency, loss, msg) = self.check_udp_port(target_address, *port, *count, *timeout_ms).await;
                CheckResult {
                    category: String::new(),
                    server_name: server.name.clone(),
                    parent_address: server.address.clone(),
                    target_address: target_address.to_string(),
                    timestamp,
                    check_type: format!("UDP:{}", port),
                    status,
                    latency_ms: latency,
                    packet_loss: Some(loss),
                    message: msg,
                    category_order: 0,
                    server_order: 0,
                    check_order: 0,
                    provider_node: None,
                }
            }
            CheckType::Http { method, expected_status, contains, timeout_ms } => {
                let url = if target_address.starts_with("http://") || target_address.starts_with("https://") {
                    target_address.to_string()
                } else {
                    format!("http://{}", target_address)
                };
                let (status, latency, msg) = self.check_http(&url, method.as_deref(), *expected_status, contains.as_deref(), *timeout_ms).await;
                CheckResult {
                    category: String::new(),
                    server_name: server.name.clone(),
                    parent_address: server.address.clone(),
                    target_address: target_address.to_string(),
                    timestamp,
                    check_type: format!("HTTP:{}", method.as_deref().unwrap_or("GET")),
                    status,
                    latency_ms: latency,
                    packet_loss: if status { Some(0.0) } else { Some(100.0) },
                    message: msg,
                    category_order: 0,
                    server_order: 0,
                    check_order: 0,
                    provider_node: None,
                }
            }
        }
    }

    async fn check_ping(&self, address: &str, count: u32, timeout_ms: u64) -> (bool, Option<f64>, Option<f64>, String) {
        let ip = match self.resolve(address).await {
            Ok(ip) => ip,
            Err(e) => return (false, None, None, format!("Domain Resolution Error: {}", e)),
        };

        let payload = [0u8; 56];
        let pinger_id = PingIdentifier(rand::random());
        let mut pinger = self.ping_client.pinger(ip, pinger_id).await;
        pinger.timeout(Duration::from_millis(timeout_ms));

        let _ = pinger.ping(PingSequence(0xFFFF), &payload).await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        let mut received = 0;
        let mut total_latency = 0.0;

        for i in 0..count {
            match pinger.ping(PingSequence(i as u16), &payload).await {
                Ok((_, latency)) => {
                    received += 1;
                    total_latency += latency.as_secs_f64() * 1000.0;
                }
                Err(_) => {}
            }
            if i < count - 1 { 
                tokio::time::sleep(Duration::from_millis(250)).await; 
            }
        }

        let loss = ((count - received) as f64 / count as f64) * 100.0;
        if received > 0 {
            (true, Some(total_latency / received as f64), Some(loss), "ICMP Connection: Verified".into())
        } else {
            (false, None, Some(100.0), "Signal Loss Detected".into())
        }
    }

    async fn raw_tcp_check(address: &str, port: u16, timeout_ms: u64) -> (bool, Option<f64>, String) {
        let addr = format!("{}:{}", address, port);
        let mut last_error = String::from("Timeout");
        
        for attempt in 0..2 {
            let start = std::time::Instant::now();
            match tokio::time::timeout(Duration::from_millis(timeout_ms), TcpStream::connect(&addr)).await {
                Ok(Ok(_)) => return (true, Some(start.elapsed().as_secs_f64() * 1000.0), "Connection Established".into()),
                Ok(Err(e)) => last_error = format!("Connection Rejected: {}", e),
                Err(_) => last_error = "Request Timeout".into(),
            }
            
            if attempt == 0 {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        (false, None, last_error)
    }

    async fn check_tcp_port(&self, address: &str, port: u16, count: u32, timeout_ms: u64) -> (bool, Option<f64>, f64, String) {
        let mut received = 0;
        let mut total_latency = 0.0;
        let mut last_error = String::from("TCP Handshake Failure");

        for i in 0..count {
            let (status, latency, msg) = Self::raw_tcp_check(address, port, timeout_ms).await;
            if status {
                received += 1;
                total_latency += latency.unwrap_or(0.0);
            } else {
                last_error = msg;
            }
            if i < count - 1 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }

        let loss = ((count - received) as f64 / count as f64) * 100.0;
        if received > 0 {
            (true, Some(total_latency / received as f64), loss, "Connection Established".into())
        } else {
            (false, None, 100.0, last_error)
        }
    }

    async fn check_udp_port(&self, address: &str, port: u16, count: u32, timeout_ms: u64) -> (bool, Option<f64>, f64, String) {
        let mut received = 0;
        let mut total_latency = 0.0;
        let mut last_error = String::from("UDP Broadcast Failure");

        for i in 0..count {
            let addr = format!("{}:{}", address, port);
            let start = std::time::Instant::now();
            let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await;
            
            match socket {
                Ok(s) => {
                    if tokio::time::timeout(Duration::from_millis(timeout_ms), s.send_to(&[], &addr)).await.is_ok() {
                        received += 1;
                        total_latency += start.elapsed().as_secs_f64() * 1000.0;
                    } else {
                        last_error = "Datagram Dropout".into();
                    }
                }
                Err(e) => last_error = format!("Socket Layer Logic Fault: {}", e),
            }
            if i < count - 1 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }

        let loss = ((count - received) as f64 / count as f64) * 100.0;
        if received > 0 {
            (true, Some(total_latency / received as f64), loss, "UDP Traffic: Active Flow".into())
        } else {
            (false, None, 100.0, last_error)
        }
    }

    async fn check_http(&self, url: &str, method: Option<&str>, expected_status: Option<u16>, contains: Option<&str>, timeout_ms: Option<u64>) -> (bool, Option<f64>, String) {
        let method_str = method.unwrap_or("GET");
        let http_method = match method_str.to_uppercase().as_str() {
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            "PATCH" => reqwest::Method::PATCH,
            _ => reqwest::Method::GET,
        };

        let timeout = Duration::from_millis(timeout_ms.unwrap_or(3500));
        let request = self.http_client.request(http_method, url).timeout(timeout).build();

        let req = match request {
            Ok(r) => r,
            Err(e) => return (false, None, format!("Request Build Error: {}", e)),
        };

        let start = std::time::Instant::now();
        match self.http_client.execute(req).await {
            Ok(response) => {
                let latency = start.elapsed().as_secs_f64() * 1000.0;
                let status_code = response.status().as_u16();
                
                let expected_stat = expected_status.unwrap_or(200);
                if status_code != expected_stat {
                    return (false, Some(latency), format!("HTTP {}, expected {}", status_code, expected_stat));
                }

                if let Some(substring) = contains {
                    match response.text().await {
                        Ok(body) => {
                            if !body.contains(substring) {
                                return (false, Some(latency), format!("Status Code {}, but body missing '{}'", status_code, substring));
                            } else {
                                return (true, Some(latency), format!("Status Code {}", status_code));
                            }
                        }
                        Err(e) => {
                            return (false, Some(latency), format!("Body read error: {}", e));
                        }
                    }
                }

                (true, Some(latency), format!("Status Code {}", status_code))
            }
            Err(e) => {
                let err_msg = if e.is_timeout() {
                    "Request Timeout".to_string()
                } else {
                    format!("Request error: {}", e)
                };
                (false, None, err_msg)
            }
        }
    }

    async fn resolve(&self, address: &str) -> Result<IpAddr, String> {
        if let Ok(ip) = address.parse::<IpAddr>() { return Ok(ip); }
        match self.dns_resolver.lookup_ip(address).await {
            Ok(lookup) => lookup.iter().next().ok_or_else(|| "Logic Fault: A-Record Resolution Error".into()),
            Err(e) => Err(format!("Resolver Context Fault (Cloudflare): {}", e)),
        }
    }

    async fn process_result(self: &Arc<Self>, result: CheckResult) {
        let key = format!("{}-{}-{}-{}", result.server_name, result.parent_address, result.target_address, result.check_type);
        let new_status = if result.status { Status::Up } else { Status::Down };
        
        if let Some(redis) = &self.redis {
            let _ = redis.push_result(&key, &result).await;
        }

        let mut state_lock = self.state.lock().await;
        let old_result = state_lock.last_results.get(&key).cloned();
        state_lock.last_results.insert(key, result.clone());
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

        let cfg = self.config.read().await;
        if cfg.webhook_url.is_some() || cfg.ntfy_topic.is_some() {
            let this = Arc::clone(self);
            tokio::spawn(async move { this.dispatch_notifications(result, old, new_status).await; });
        }
    }

    async fn dispatch_notifications(&self, result: CheckResult, old: Status, new: Status) {
        info!("Dispatching infrastructure event notification: {} -> {:?}", result.server_name, new);
        
        let cfg = self.config.read().await;
        if let Some(topic) = &cfg.ntfy_topic {
            let priority = if new == Status::Down { "5" } else { "3" };
            let title = format!("{} -> {:?}", result.target_address, new);
            let body = format!("{}: {}", result.check_type, result.message);
            
            let req = self.http_client.post(format!("https://ntfy.sh/{}", topic))
                .header("Title", title)
                .header("Priority", priority)
                .header("Tags", if new == Status::Down { "warning,computer" } else { "heavy_check_mark" })
                .header("User-Agent", "SPECTRA-Monitor/3.1.0")
                .body(body);

            match req.send().await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        error!("Telemetry transmission rejected by ntfy.sh: Status {}", resp.status());
                    }
                }
                Err(e) => error!("Failed to transmit ntfy.sh telemetry: {}", e),
            }
        }

        if let Some(url) = &cfg.webhook_url {
            if url.contains("discord.com") {
                self.send_discord_webhook(url, result, old, new).await;
            } else {
                self.send_generic_webhook(url, result, old, new).await;
            }
        }
    }

    async fn send_discord_webhook(&self, url: &str, result: CheckResult, old: Status, new: Status) {
        let color = if new == Status::Up { 0x2ECC71 } else { 0xE74C3C };
        let payload = serde_json::json!({
            "username": "SPECTRA Engine",
            "embeds": [{
                "title": "Mesh Protocol Transition",
                "color": color,
                "fields": [
                    { "name": "Cluster", "value": result.server_name, "inline": true },
                    { "name": "Resource IP", "value": result.target_address, "inline": true },
                    { "name": "Transition", "value": format!("{:?} \u{2192} {:?}", old, new), "inline": true },
                    { "name": "Protocol", "value": result.check_type, "inline": true },
                    { "name": "Telemetry", "value": result.latency_ms.map_or("N/A".to_string(), |l| format!("{:.2}ms", l)), "inline": true },
                    { "name": "Diagnosis", "value": result.message.to_uppercase(), "inline": false }
                ],
                "timestamp": Utc::now().to_rfc3339(),
                "footer": { "text": "SPECTRA Infrastructure Intelligence" }
            }]
        });
        let _ = self.http_client.post(url).json(&payload).send().await;
    }

    async fn send_generic_webhook(&self, url: &str, result: CheckResult, _old: Status, new: Status) {
        let payload = serde_json::json!({
            "text": format!("SPECTRA Alert: {} ({}) is now {:?}", result.server_name, result.target_address, new)
        });
        let _ = self.http_client.post(url).json(&payload).send().await;
    }

    pub async fn shutdown(&self) {
        if let Some(redis) = &self.redis {
            let node_id = self.state.lock().await.node_id.clone();
            info!("Gracefully unregistering node {} from cluster...", node_id);
            let _ = redis.unregister_node(&node_id).await;
        }
    }
}
