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
    pub parent_address: String,
    pub target_address: String,
    pub timestamp: DateTime<Utc>,
    pub check_type: String,
    pub status: bool,
    pub latency_ms: Option<f64>,
    pub packet_loss: Option<f64>,
    pub message: String,
    pub category_order: usize,
    pub server_order: usize,
    pub check_order: usize,
    pub provider_node: Option<String>,
}

impl CheckResult {
    pub fn mask_addresses(&mut self) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let old_target = self.target_address.clone();
        let old_parent = self.parent_address.clone();

        let mut hasher = DefaultHasher::new();
        self.target_address.hash(&mut hasher);
        self.target_address = format!("HIDDEN-{}", &format!("{:x}", hasher.finish())[..6].to_uppercase());

        let mut p_hasher = DefaultHasher::new();
        self.parent_address.hash(&mut p_hasher);
        self.parent_address = format!("HIDDEN-{}", &format!("{:x}", p_hasher.finish())[..6].to_uppercase());

        self.message = self.message.replace(&old_target, &self.target_address);
        if old_parent != old_target {
            self.message = self.message.replace(&old_parent, &self.parent_address);
        }
    }
}

pub struct MonitorState {
    pub last_results: HashMap<String, CheckResult>,
    pub node_id: String,
    pub live_nodes: Vec<String>,
}
