use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct LnchConfig {
    pub name: String,
    pub tasks: Vec<TaskConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskConfig {
    pub name: String,
    pub command: String,
    pub working_dir: Option<PathBuf>,
    pub env: Option<HashMap<String, String>>,
    pub color: Option<String>,
    pub depends_on: Option<Vec<String>>,
}
