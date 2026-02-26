use anyhow::Context;
use std::path::{Path, PathBuf};

use super::model::LnchConfig;

const MAX_SEARCH_DEPTH: usize = 10;
const CONFIG_FILENAME: &str = "lnch.yaml";

/// Search upward from the current directory for lnch.yaml
pub fn find_config() -> anyhow::Result<PathBuf> {
    let mut current = std::env::current_dir()?;

    for _ in 0..MAX_SEARCH_DEPTH {
        let candidate = current.join(CONFIG_FILENAME);
        if candidate.exists() {
            return Ok(candidate);
        }
        if !current.pop() {
            break;
        }
    }

    anyhow::bail!(
        "lnch.yaml not found.\n\
         Run 'lnch init' to create one, or specify a file with 'lnch --config <path>'."
    )
}

/// Load and deserialize a YAML config file
pub fn load_config(path: &Path) -> anyhow::Result<LnchConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let config: LnchConfig = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
    Ok(config)
}

/// Resolve the config base directory (parent of the config file)
pub fn config_base_dir(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolve relative `working_dir` paths in all tasks to absolute paths based on `base_dir`.
///
/// Without this, `cmd.current_dir()` in TaskRunner resolves relative paths against the
/// process CWD, which differs from the config file's directory when `lnch` is invoked
/// from a subdirectory and the config is found via upward search.
pub fn resolve_working_dirs(config: &mut LnchConfig, base_dir: &Path) {
    for task in &mut config.tasks {
        if let Some(ref dir) = task.working_dir {
            if !dir.is_absolute() {
                task.working_dir = Some(base_dir.join(dir));
            }
        }
    }
}
