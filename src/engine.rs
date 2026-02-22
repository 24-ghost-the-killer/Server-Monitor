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

                            // Pacing: Add a small delay between spawns to prevent network saturation/throttling
                            // Especially important when scanning subnets (e.g. /24)
                            tokio::time::sleep(Duration::from_millis(10)).await;
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
                            latency = p_latency; 
                            loss = Some(0.0);
                            msg = format!("ICMP Filtered (Handshake verified via {} - {}ms)", 
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
                tokio::time::sleep(Duration::from_millis(100)).await; 
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
        let start = std::time::Instant::now();
        match tokio::time::timeout(Duration::from_millis(timeout_ms), TcpStream::connect(&addr)).await {
            Ok(Ok(_)) => (true, Some(start.elapsed().as_secs_f64() * 1000.0), "Connection Established".into()),
            Ok(Err(e)) => (false, None, format!("Connection Rejected: {}", e)),
            Err(_) => (false, None, "Request Timeout".into()),
        }
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
                tokio::time::sleep(Duration::from_millis(50)).await;
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
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        let loss = ((count - received) as f64 / count as f64) * 100.0;
        if received > 0 {
            (true, Some(total_latency / received as f64), loss, "UDP Traffic: Active Flow".into())
        } else {
            (false, None, 100.0, last_error)
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
        
        let mut state_lock = self.state.lock().await;
        let old_result = state_lock.last_results.get(&key).cloned();
        state_lock.last_results.insert(key, result.clone());
        drop(state_lock);

        let is_awaiting = old_result.as_ref().map_or(true, |r| r.message == "Awaiting Infrastructure Handshake...");
        let old_status = old_result.map(|r| if r.status { Status::Up } else { Status::Down });

        let old = match old_status {
            Some(old) if old != new_status => Some(old),
            // If we find a node is DOWN on the first check, we MUST notify (simulating Up -> Down)
            _ if is_awaiting && new_status == Status::Down => Some(Status::Up),
            _ => None,
        };

        let old = match old {
            Some(o) => o,
            None => return,
        };

        let msg = format!("[CHANGE] {}/{} ({}) -> {:?}", result.server_name, result.check_type, result.target_address, new_status);
        if new_status == Status::Down { error!("{}", msg); } else { warn!("{}", msg); }

        // Mute the "Initial UP" notifications to avoid startup spam
        if is_awaiting && new_status == Status::Up {
            return;
        }

        if self.config.webhook_url.is_some() || self.config.ntfy_topic.is_some() {
            let this = Arc::clone(self);
            tokio::spawn(async move { this.dispatch_notifications(result, old, new_status).await; });
        }
    }

    async fn dispatch_notifications(&self, result: CheckResult, old: Status, new: Status) {
        info!("Dispatching infrastructure event notification: {} -> {:?}", result.server_name, new);
        
        // Handle ntfy.sh (Native Phone Push)
        if let Some(topic) = &self.config.ntfy_topic {
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

        // Handle Webhooks (Discord/Slack/Generic)
        if let Some(url) = &self.config.webhook_url {
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
}
