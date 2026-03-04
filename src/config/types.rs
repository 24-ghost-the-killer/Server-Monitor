use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MonitorConfig {
    pub categories: Vec<Category>,
    pub check_interval: u64,
    pub webhook_url: Option<String>,
    pub ntfy_topic: Option<String>,
    #[serde(default = "default_packet_loss_threshold")]
    pub packet_loss_threshold: f64,
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: usize,
    #[serde(default)]
    pub hide_endpoints: bool,
    #[serde(default)]
    pub redis_url: Option<String>,
    #[serde(default = "default_redis_prefix")]
    pub redis_prefix: String,
    #[serde(default = "default_max_checks_per_second")]
    pub max_checks_per_second: u64,
    #[serde(default)]
    pub enable_dashboard: Option<bool>,
}




pub fn default_packet_loss_threshold() -> f64 { 101.0 }
pub fn default_redis_prefix() -> String { "spectra".into() }
pub fn default_max_checks_per_second() -> u64 { 50 }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Category {
    pub name: String,
    pub servers: Vec<Server>,
}

pub fn default_api_port() -> u16 { 3000 }
pub fn default_max_concurrency() -> usize { 1500 }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Server {
    pub name: String,
    pub address: String,
    pub checks: Vec<CheckType>,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default)]
    pub packet_loss_threshold: Option<f64>,
}



pub fn default_max_retries() -> u32 { 1 }

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum CheckType {
    Ping {
        #[serde(default = "default_ping_count")]
        count: u32,
        #[serde(default = "default_timeout")]
        timeout_ms: u64,
        #[serde(default)]
        simulate_loss: Option<f64>,
    },
    TcpPort {
        port: u16,
        #[serde(default = "default_ping_count")]
        count: u32,
        #[serde(default = "default_timeout")]
        timeout_ms: u64,
        #[serde(default)]
        simulate_loss: Option<f64>,
    },
    UdpPort {
        port: u16,
        #[serde(default = "default_ping_count")]
        count: u32,
        #[serde(default = "default_timeout")]
        timeout_ms: u64,
        #[serde(default)]
        simulate_loss: Option<f64>,
    },
    Http {
        #[serde(default = "default_http_method")]
        method: Option<String>,
        #[serde(default = "default_http_status")]
        expected_status: Option<u16>,
        contains: Option<String>,
        #[serde(default = "default_http_timeout")]
        timeout_ms: Option<u64>,
    },
}

pub fn default_ping_count() -> u32 { 1 }
pub fn default_timeout() -> u64 { 3500 }

pub fn default_http_method() -> Option<String> { Some("GET".to_string()) }
pub fn default_http_status() -> Option<u16> { Some(200) }
pub fn default_http_timeout() -> Option<u64> { Some(3500) }
