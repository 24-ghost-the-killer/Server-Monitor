use crate::models::{CheckResult, Status};
use crate::engine::Monitor;
use tracing::{info, error};
use chrono::Utc;


impl Monitor {
    pub async fn dispatch_notifications(&self, mut result: CheckResult, old: Status, new: Status) {
        if self.config.read().await.hide_endpoints {
            result.mask_addresses();
        }
        
        info!("Dispatching infrastructure event notification: {} -> {:?}", result.server_name, new);
        
        let cfg = self.config.read().await;
        if let Some(topic) = &cfg.ntfy_topic {
            let priority = if new == Status::Down { "5" } else { "3" };
            let title = format!("{} -> {:?}", result.server_name, new);
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

    pub async fn send_discord_webhook(&self, url: &str, result: CheckResult, old: Status, new: Status) {
        let color = if new == Status::Up { 0x2ECC71 } else { 0xE74C3C };
        let display_addr = if result.target_address.starts_with("HIDDEN-") {
            result.server_name.clone()
        } else {
            result.target_address.clone()
        };

        let fields = vec![
            serde_json::json!({ "name": "Cluster", "value": result.server_name, "inline": true }),
            serde_json::json!({ "name": "Resource", "value": display_addr, "inline": true }),
            serde_json::json!({ "name": "Transition", "value": format!("{:?} \u{2192} {:?}", old, new), "inline": true }),
            serde_json::json!({ "name": "Protocol", "value": result.check_type, "inline": true }),
            serde_json::json!({ "name": "Telemetry", "value": result.latency_ms.map_or("N/A".to_string(), |l| format!("{:.2}ms", l)), "inline": true }),
            serde_json::json!({ "name": "Diagnosis", "value": result.message.to_uppercase(), "inline": false })
        ];

        let payload = serde_json::json!({
            "username": "SPECTRA Engine",
            "embeds": [{
                "title": "Mesh Protocol Transition",
                "color": color,
                "fields": fields,
                "timestamp": Utc::now().to_rfc3339(),
                "footer": { "text": "SPECTRA Infrastructure Intelligence" }
            }]
        });
        let _ = self.http_client.post(url).json(&payload).send().await;
    }

    pub async fn send_generic_webhook(&self, url: &str, result: CheckResult, _old: Status, new: Status) {
        let text = if result.target_address.starts_with("HIDDEN-") {
            format!("SPECTRA Alert: {} is now {:?}", result.server_name, new)
        } else {
            format!("SPECTRA Alert: {} ({}) is now {:?}", result.server_name, result.target_address, new)
        };
        let payload = serde_json::json!({
            "text": text
        });
        let _ = self.http_client.post(url).json(&payload).send().await;
    }
}
