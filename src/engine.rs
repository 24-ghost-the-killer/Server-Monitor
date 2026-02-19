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
use tokio::sync::{Mutex, Semaphore};
use tracing::{error, info, warn};

use crate::config::{MonitorConfig, Server, CheckType};
use crate::models::{CheckResult, MonitorState, Status};

pub struct Monitor {
    pub config: MonitorConfig,
    ping_client: PingClient,
    pub state: Arc<Mutex<MonitorState>>,
    http_client: reqwest::Client,
    concurrency_limiter: Arc<Semaphore>,
    dns_resolver: TokioResolver,
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

        Ok(Self {
            config,
            ping_client,
            state: Arc::new(Mutex::new(MonitorState {
                last_results: HashMap::new(),
            })),
            http_client: reqwest::Client::new(),
            concurrency_limiter: Arc::new(Semaphore::new(max_concurrent)),
            dns_resolver,
        })
    }

    pub async fn initialize_state(&self) {
        info!("Initializing infrastructure mesh state...");
        let mut state = self.state.lock().await;
        let now = Utc::now();

        for category in &self.config.categories {
            for server in &category.servers {
                let addresses = if let Ok(net) = server.address.parse::<IpNet>() {
                    net.hosts().map(|ip| ip.to_string()).collect::<Vec<_>>()
                } else {
                    vec![server.address.clone()]
                };

                for address in addresses {
                    for check in &server.checks {
                        let check_type_name = match check {
                            CheckType::Ping { .. } => "Ping".into(),
                            CheckType::TcpPort { port, .. } => format!("TCP:{}", port),
                            CheckType::UdpPort { port, .. } => format!("UDP:{}", port),
                        };
                        
                        let key = format!("{}-{}-{}", server.name, address, check_type_name);
                        state.last_results.entry(key).or_insert(CheckResult {
                            category: category.name.clone(),
                            server_name: server.name.clone(),
                            target_address: address.clone(),
                            timestamp: now,
                            check_type: check_type_name,
                            status: false,
                            latency_ms: None,
                            packet_loss: None,
                            message: "Synchronizing status...".into(),
                        });
                    }
                }
            }
        }
        info!("Mesh state warmed with {} tracking points", state.last_results.len());
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        info!("Engine V4: Turbo Mesh Monitoring Active...");
        info!("--- Max Concurrency: {} workers ---", self.config.max_concurrency);
        
        self.initialize_state().await;
        
        loop {
            let start_time = Utc::now();
            let mut tasks = FuturesUnordered::new();

            for category in &self.config.categories {
                for server in &category.servers {
                    let addresses = if let Ok(net) = server.address.parse::<IpNet>() {
                        net.hosts().map(|ip| ip.to_string()).collect::<Vec<_>>()
                    } else {
                        vec![server.address.clone()]
                    };

                    for address in addresses {
                        for check in &server.checks {
                            let monitor_ref = Arc::clone(&self);
                            let s_clone = server.clone();
                            let a_clone = address.clone();
                            let c_clone = check.clone();
                            let cat_name = category.name.clone();
                            
                            tasks.push(tokio::spawn(async move {
                                let _permit = monitor_ref.concurrency_limiter.acquire().await.ok();
                                let mut res = monitor_ref.run_check_with_retry(s_clone, a_clone, c_clone).await;
                                res.category = cat_name;
                                res
                            }));
                        }
                    }
                }
            }

            let mut results = Vec::with_capacity(tasks.len());
            let total = tasks.len();
            while let Some(join_res) = tasks.next().await {
                if let Ok(result) = join_res {
                    results.push(result);
                }
            }

            results.sort_by(|a, b| {
                a.category.cmp(&b.category)
                    .then(a.server_name.cmp(&b.server_name))
                    .then(a.check_type.cmp(&b.check_type))
            });

            for result in results {
                self.process_result(result).await;
            }

            let duration = Utc::now() - start_time;
            info!("Turbo cycle completed {} checks in {:.2}s.", 
                total,
                duration.num_milliseconds() as f64 / 1000.0);

            tokio::time::sleep(Duration::from_secs(self.config.check_interval)).await;
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
 
                            latency = None; 
                            loss = Some(0.0);
                            msg = format!("ICMP Filtered (Verified via {} [{}ms])", 
                                name, 
                                p_latency.map_or("N/A".to_string(), |l| format!("{:.1}", l)));
                            break;
                        }
                    }
                }

                CheckResult {
                    category: String::new(),
                    server_name: server.name.clone(),
                    target_address: target_address.to_string(),
                    timestamp,
                    check_type: "Ping".into(),
                    status,
                    latency_ms: latency,
                    packet_loss: loss,
                    message: msg,
                }
            }
            CheckType::TcpPort { port, timeout_ms } => {
                let (status, latency, msg) = self.check_tcp_port(target_address, *port, *timeout_ms).await;
                CheckResult {
                    category: String::new(),
                    server_name: server.name.clone(),
                    target_address: target_address.to_string(),
                    timestamp,
                    check_type: format!("TCP:{}", port),
                    status,
                    latency_ms: latency,
                    packet_loss: None,
                    message: msg,
                }
            }
            CheckType::UdpPort { port, timeout_ms: _ } => {
                let (status, latency, msg) = self.check_udp_port(target_address, *port).await;
                CheckResult {
                    category: String::new(),
                    server_name: server.name.clone(),
                    target_address: target_address.to_string(),
                    timestamp,
                    check_type: format!("UDP:{}", port),
                    status,
                    latency_ms: latency,
                    packet_loss: None,
                    message: msg,
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

        for i in 0..count {
            match pinger.ping(PingSequence(i as u16), &payload).await {
                Ok((_, latency)) => {
                    let lat_ms = latency.as_secs_f64() * 1000.0;
                    return (true, Some(lat_ms), Some(0.0), "ICMP Response OK".into());
                }
                Err(_) => {
                    if i < count - 1 { 
                        tokio::time::sleep(Duration::from_millis(50)).await; 
                    }
                }
            }
        }
        (false, None, Some(100.0), "Request Timeout (Packet Loss 100%)".into())
    }

    async fn raw_tcp_check(address: &str, port: u16, timeout_ms: u64) -> (bool, Option<f64>, String) {
        let addr = format!("{}:{}", address, port);
        let start = std::time::Instant::now();
        match tokio::time::timeout(Duration::from_millis(timeout_ms), TcpStream::connect(&addr)).await {
            Ok(Ok(_)) => (true, Some(start.elapsed().as_secs_f64() * 1000.0), "TCP Handshake Success".into()),
            Ok(Err(e)) => (false, None, format!("Connection Refused: {}", e)),
            Err(_) => (false, None, "Port Timeout".into()),
        }
    }

    async fn check_tcp_port(&self, address: &str, port: u16, timeout_ms: u64) -> (bool, Option<f64>, String) {
        Self::raw_tcp_check(address, port, timeout_ms).await
    }

    async fn check_udp_port(&self, address: &str, port: u16) -> (bool, Option<f64>, String) {
        let addr = format!("{}:{}", address, port);
        let start = std::time::Instant::now();
        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await;
        match socket {
            Ok(s) => {
                if s.send_to(&[], &addr).await.is_ok() { (true, Some(start.elapsed().as_secs_f64() * 1000.0), "UDP Probe Transmitted".into()) } 
                else { (false, None, "UDP Broadcast Failure".into()) }
            }
            Err(e) => (false, None, format!("Local Socket Error: {}", e)),
        }
    }

    async fn resolve(&self, address: &str) -> Result<IpAddr, String> {
        if let Ok(ip) = address.parse::<IpAddr>() { return Ok(ip); }
        match self.dns_resolver.lookup_ip(address).await {
            Ok(lookup) => lookup.iter().next().ok_or_else(|| "No IP Address Found".into()),
            Err(e) => Err(format!("Cloudflare DNS Resolution Failed: {}", e)),
        }
    }

    async fn process_result(self: &Arc<Self>, result: CheckResult) {
        let key = format!("{}-{}-{}", result.server_name, result.target_address, result.check_type);
        let new_status = if result.status { Status::Up } else { Status::Down };
        
        let mut state_lock = self.state.lock().await;
        let old_status = state_lock.last_results.get(&key).map(|r| if r.status { Status::Up } else { Status::Down });
        state_lock.last_results.insert(key, result.clone());
        drop(state_lock);

        let old = match old_status {
            Some(old) if old != new_status => old,
            None if new_status == Status::Down => Status::Up,
            _ => return,
        };

        let msg = format!("[CHANGE] {}/{} ({}) -> {:?}", result.server_name, result.check_type, result.target_address, new_status);
        if new_status == Status::Down { error!("{}", msg); } else { warn!("{}", msg); }

        if self.config.webhook_url.is_some() {
            let this = Arc::clone(self);
            tokio::spawn(async move { this.send_webhook(result, old, new_status).await; });
        }
    }

    async fn send_webhook(&self, result: CheckResult, old: Status, new: Status) {
        if let Some(url) = &self.config.webhook_url {
            let color = if new == Status::Up { 0x2ECC71 } else { 0xE74C3C };
            let payload = serde_json::json!({
                "username": "NetPulse Engine",
                "embeds": [{
                    "title": "Protocol Status Transition",
                    "color": color,
                    "fields": [
                        { "name": "Cluster", "value": result.server_name, "inline": true },
                        { "name": "Node IP", "value": result.target_address, "inline": true },
                        { "name": "Transition", "value": format!("{:?} \u{2192} {:?}", old, new), "inline": true },
                        { "name": "Metric", "value": result.check_type, "inline": true },
                        { "name": "Sync Latency", "value": result.latency_ms.map_or("N/A".to_string(), |l| format!("{:.2}ms", l)), "inline": true },
                        { "name": "Reason", "value": result.message, "inline": false }
                    ],
                    "timestamp": Utc::now().to_rfc3339(),
                    "footer": { "text": "NetPulse Infrastructure Intelligence" }
                }]
            });
            let _ = self.http_client.post(url).json(&payload).send().await;
        }
    }
}
