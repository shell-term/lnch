use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum LnchError {
    #[error("Config file not found")]
    ConfigNotFound,

    #[error("Failed to parse config: {0}")]
    ConfigParse(#[from] serde_yaml::Error),

    #[error("Config validation error: {0}")]
    ConfigValidation(String),

    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    #[error("Failed to start task '{task}': {source}")]
    TaskStart {
        task: String,
        source: std::io::Error,
    },

    #[error("Terminal initialization failed: {0}")]
    TerminalInit(std::io::Error),
}
