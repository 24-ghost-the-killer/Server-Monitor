use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Status {
    Up,
    Down,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub category: String,
    pub server_name: String,
    pub target_address: String,
    pub timestamp: DateTime<Utc>,
    pub check_type: String,
    pub status: bool,
    pub latency_ms: Option<f64>,
    pub packet_loss: Option<f64>,
    pub message: String,
}

pub struct MonitorState {
    pub last_results: HashMap<String, CheckResult>,
}
