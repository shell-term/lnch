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
    pub ready_check: Option<ReadyCheckConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReadyCheckConfig {
    pub tcp: Option<TcpCheck>,
    pub http: Option<HttpCheck>,
    pub log_line: Option<LogLineCheck>,
    pub exit: Option<ExitCheck>,
    /// Timeout in seconds (default: 30)
    pub timeout: Option<u64>,
    /// Polling interval in milliseconds (default: 500)
    pub interval: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TcpCheck {
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpCheck {
    pub url: String,
    pub status: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogLineCheck {
    pub pattern: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExitCheck {}

impl ReadyCheckConfig {
    /// Count how many check types are set.
    pub fn check_type_count(&self) -> usize {
        self.tcp.is_some() as usize
            + self.http.is_some() as usize
            + self.log_line.is_some() as usize
            + self.exit.is_some() as usize
    }
}
