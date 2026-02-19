use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MonitorConfig {
    pub categories: Vec<Category>,
    pub check_interval: u64,
    pub webhook_url: Option<String>,
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Category {
    pub name: String,
    pub servers: Vec<Server>,
}

fn default_api_port() -> u16 { 3000 }
fn default_max_concurrency() -> usize { 1500 }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Server {
    pub name: String,
    pub address: String,
    pub checks: Vec<CheckType>,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_max_retries() -> u32 { 1 }

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum CheckType {
    Ping {
        #[serde(default = "default_ping_count")]
        count: u32,
        #[serde(default = "default_timeout")]
        timeout_ms: u64,
    },
    TcpPort {
        port: u16,
        #[serde(default = "default_timeout")]
        timeout_ms: u64,
    },
    UdpPort {
        port: u16,
        #[serde(default = "default_timeout")]
        timeout_ms: u64,
    },
}

pub fn default_ping_count() -> u32 { 1 }
pub fn default_timeout() -> u64 { 3500 }
