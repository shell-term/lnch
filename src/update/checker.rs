use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const GITHUB_API_URL: &str =
    "https://api.github.com/repos/shell-term/lnch/releases/latest";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60; // 24 hours
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// -- Public types -----------------------------------------------------------

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub latest_version: String,
}

impl UpdateInfo {
    /// The shell command the user would run to install this version.
    pub fn install_command(&self) -> String {
        let v = &self.latest_version;
        if cfg!(windows) {
            let ps = find_powershell();
            format!(
                "{ps} -ExecutionPolicy ByPass -c \"irm https://github.com/shell-term/lnch/releases/download/v{v}/lnch-installer.ps1 | iex\""
            )
        } else {
            format!(
                "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/shell-term/lnch/releases/download/v{v}/lnch-installer.sh | sh"
            )
        }
    }
}

// -- Cache ------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct UpdateCache {
    last_check_epoch: u64,
    latest_version: String,
}

impl UpdateCache {
    fn is_fresh(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.last_check_epoch) < CACHE_TTL_SECS
    }
}

fn cache_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var("LOCALAPPDATA").ok().map(PathBuf::from)
    } else {
        std::env::var("XDG_CACHE_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".cache")))
    }?;
    Some(base.join("lnch").join("update_check.json"))
}

fn read_cache() -> Option<UpdateCache> {
    let path = cache_path()?;
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_cache(latest_version: &str) {
    let Some(path) = cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cache = UpdateCache {
        last_check_epoch: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        latest_version: latest_version.to_string(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = std::fs::write(path, json);
    }
}

// -- Version comparison -----------------------------------------------------

fn is_newer(current: &str, latest: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.').filter_map(|s| s.parse().ok()).collect()
    };
    parse(latest) > parse(current)
}

// -- GitHub API fetch -------------------------------------------------------

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

async fn fetch_latest_version() -> Option<String> {
    let output = tokio::process::Command::new("curl")
        .args([
            "-s",
            "-H",
            "Accept: application/vnd.github.v3+json",
            "-H",
            "User-Agent: lnch-update-checker",
            GITHUB_API_URL,
        ])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let release: GitHubRelease = serde_json::from_slice(&output.stdout).ok()?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    Some(version.to_string())
}

// -- Public entry point -----------------------------------------------------

/// Check for a newer release in the background.
/// Returns `Some(UpdateInfo)` if a newer version is available, `None` otherwise.
/// Silently returns `None` on any error (network, parse, etc.).
pub async fn check_for_update() -> Option<UpdateInfo> {
    // Opt-out via environment variable
    if std::env::var("LNCH_NO_UPDATE_CHECK").is_ok() {
        return None;
    }

    // Try cached result first
    if let Some(cached) = read_cache() {
        if cached.is_fresh() {
            return if is_newer(CURRENT_VERSION, &cached.latest_version) {
                Some(UpdateInfo {
                    latest_version: cached.latest_version,
                })
            } else {
                None
            };
        }
    }

    // Fetch from GitHub
    let latest = fetch_latest_version().await?;
    write_cache(&latest);

    if is_newer(CURRENT_VERSION, &latest) {
        Some(UpdateInfo {
            latest_version: latest,
        })
    } else {
        None
    }
}

/// Return "pwsh" if PowerShell 7+ is on PATH, otherwise "powershell".
#[cfg(windows)]
fn find_powershell() -> &'static str {
    use std::sync::OnceLock;
    static PS: OnceLock<&str> = OnceLock::new();
    *PS.get_or_init(|| {
        match std::process::Command::new("pwsh")
            .arg("-Version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Ok(s) if s.success() => "pwsh",
            _ => "powershell",
        }
    })
}

/// Execute the update installer. Call this after the TUI has been torn down
/// so that installer output is visible in the terminal.
pub fn execute_update(info: &UpdateInfo) {
    println!();
    println!("  Updating lnch to v{}...", info.latest_version);
    println!("  > {}", info.install_command());
    println!();

    let status = if cfg!(windows) {
        let ps = find_powershell();
        let ps_script = format!(
            "irm https://github.com/shell-term/lnch/releases/download/v{}/lnch-installer.ps1 | iex",
            info.latest_version
        );
        std::process::Command::new(ps)
            .args(["-ExecutionPolicy", "ByPass", "-Command", &ps_script])
            .status()
    } else {
        std::process::Command::new("sh")
            .args(["-c", &info.install_command()])
            .status()
    };

    match status {
        Ok(s) if s.success() => {
            println!();
            println!("  lnch has been updated to v{}!", info.latest_version);
            println!();
            println!("  ** Please restart lnch to apply the update. **");
            println!();
        }
        _ => {
            println!();
            println!("  Update failed. You can try manually:");
            println!("  {}", info.install_command());
            println!();
        }
    }
}

// -- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_patch() {
        assert!(is_newer("0.1.7", "0.1.8"));
    }

    #[test]
    fn test_is_newer_minor() {
        assert!(is_newer("0.1.7", "0.2.0"));
    }

    #[test]
    fn test_is_newer_major() {
        assert!(is_newer("0.1.7", "1.0.0"));
    }

    #[test]
    fn test_not_newer_same() {
        assert!(!is_newer("0.1.7", "0.1.7"));
    }

    #[test]
    fn test_not_newer_older() {
        assert!(!is_newer("0.1.8", "0.1.7"));
    }

    #[test]
    fn test_install_command_contains_version() {
        let info = UpdateInfo {
            latest_version: "0.2.0".to_string(),
        };
        let cmd = info.install_command();
        assert!(cmd.contains("0.2.0"), "command should contain version");
        if cfg!(windows) {
            assert!(
                cmd.contains("pwsh") || cmd.contains("powershell"),
                "command should contain pwsh or powershell"
            );
            assert!(cmd.contains(".ps1"));
        } else {
            assert!(cmd.contains("curl"));
            assert!(cmd.contains(".sh"));
        }
    }

    #[test]
    fn test_parse_github_response() {
        let json = r#"{"tag_name":"v0.2.0","name":"v0.2.0"}"#;
        let release: GitHubRelease = serde_json::from_str(json).unwrap();
        let version = release.tag_name.strip_prefix('v').unwrap();
        assert_eq!(version, "0.2.0");
    }
}
