use std::time::Duration;
use std::net::IpAddr;
use chrono::Utc;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::net::TcpStream;
use surge_ping::{PingIdentifier, PingSequence};
use crate::config::{Server, CheckType};
use crate::models::CheckResult;
use crate::engine::Monitor;

impl Monitor {
    pub async fn run_check_with_retry(&self, server: Server, target_address: String, check: CheckType) -> CheckResult {
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

    pub async fn perform_check(&self, server: &Server, target_address: &str, check: &CheckType) -> CheckResult {
        let timestamp = Utc::now();
        match check {
            CheckType::Ping { count, timeout_ms, simulate_loss } => {
                let (mut status, mut latency, mut loss, mut msg) = self.check_ping(target_address, *count, *timeout_ms, *simulate_loss).await;
                
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
                            msg = format!("Blocked (Handshake verified via {} - {}ms)", 
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
                    check_type: "ICMP".into(),
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
            CheckType::TcpPort { port, count, timeout_ms, simulate_loss } => {
                let (status, latency, loss, msg) = self.check_tcp_port(target_address, *port, *count, *timeout_ms, *simulate_loss).await;
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
            CheckType::UdpPort { port, count, timeout_ms, simulate_loss } => {
                let (status, latency, loss, msg) = self.check_udp_port(target_address, *port, *count, *timeout_ms, *simulate_loss).await;
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

    pub async fn check_ping(&self, address: &str, count: u32, timeout_ms: u64, simulate_loss: Option<f64>) -> (bool, Option<f64>, Option<f64>, String) {
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
            if let Some(sim_loss) = simulate_loss {
                if rand::random::<f64>() * 100.0 < sim_loss {
                    continue;
                }
            }
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
        
        let final_loss = if let Some(sim) = simulate_loss {
             if sim > loss { sim } else { loss }
        } else {
            loss
        };

        if received > 0 {
            let msg = if final_loss > 0.0 {
                format!("Connection: Verified ({:.1}% Loss)", final_loss).replace(".0%", "%")
            } else {
                "Connection: Verified".into()
            };
            (true, Some(total_latency / received as f64), Some(final_loss), msg)
        } else {
            let msg = if final_loss < 100.0 {
                 format!("Signal Loss Detected ({:.1}% Loss)", final_loss).replace(".0%", "%")
            } else {
                 "Signal Loss Detected (100% Loss)".into()
            };
            (false, None, Some(final_loss), msg)
        }
    }

    pub async fn raw_tcp_check(address: &str, port: u16, timeout_ms: u64) -> (bool, Option<f64>, String) {
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

    pub async fn check_tcp_port(&self, address: &str, port: u16, count: u32, timeout_ms: u64, simulate_loss: Option<f64>) -> (bool, Option<f64>, f64, String) {
        let mut received = 0;
        let mut total_latency = 0.0;
        let mut last_error = String::from("Connection Rejected");

        for i in 0..count {
            if let Some(sim_loss) = simulate_loss {
                if rand::random::<f64>() * 100.0 < sim_loss {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            }
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
        let final_loss = if let Some(sim) = simulate_loss {
            if sim > loss { sim } else { loss }
        } else {
            loss
        };

        if received > 0 {
            let msg = if final_loss > 0.0 {
                format!("Connection Established ({:.1}% Loss)", final_loss).replace(".0%", "%")
            } else {
                "Connection Established".into()
            };
            (true, Some(total_latency / received as f64), final_loss, msg)
        } else {
            let msg = if final_loss < 100.0 {
                format!("{} ({:.1}% Loss)", last_error, final_loss).replace(".0%", "%")
            } else {
                format!("{} (100% Loss)", last_error)
            };
            (false, None, final_loss, msg)
        }
    }

    pub async fn check_udp_port(&self, address: &str, port: u16, count: u32, timeout_ms: u64, simulate_loss: Option<f64>) -> (bool, Option<f64>, f64, String) {
        let mut received = 0;
        let mut total_latency = 0.0;
        let mut last_error = String::from("Connection Rejected");

        for i in 0..count {
            if let Some(sim_loss) = simulate_loss {
                if rand::random::<f64>() * 100.0 < sim_loss {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            }
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
        let final_loss = if let Some(sim) = simulate_loss {
            if sim > loss { sim } else { loss }
        } else {
            loss
        };

        if received > 0 {
            let msg = if final_loss > 0.0 {
                format!("Traffic: Active Flow ({:.1}% Loss)", final_loss).replace(".0%", "%")
            } else {
                "Traffic: Active Flow".into()
            };
            (true, Some(total_latency / received as f64), final_loss, msg)
        } else {
             let msg = if final_loss < 100.0 {
                format!("{} ({:.1}% Loss)", last_error, final_loss).replace(".0%", "%")
            } else {
                format!("{} (100% Loss)", last_error)
            };
            (false, None, final_loss, msg)
        }
    }

    pub async fn check_http(&self, url: &str, method: Option<&str>, expected_status: Option<u16>, contains: Option<&str>, timeout_ms: Option<u64>) -> (bool, Option<f64>, String) {
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
                    return (false, Some(latency), format!("Status Code {}, expected {}", status_code, expected_stat));
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

    pub async fn resolve(&self, address: &str) -> Result<IpAddr, String> {
        if let Ok(ip) = address.parse::<IpAddr>() { return Ok(ip); }
        match self.dns_resolver.lookup_ip(address).await {
            Ok(lookup) => lookup.iter().next().ok_or_else(|| "Logic Fault: A-Record Resolution Error".into()),
            Err(e) => Err(format!("Resolver Context Fault (Cloudflare): {}", e)),
        }
    }
}
