use std::io::Write;
use std::process::{Command, Stdio};

/// Copy text to the system clipboard.
/// Silently logs a warning on failure rather than crashing.
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let result = copy_platform(text);
    if let Err(ref e) = result {
        tracing::warn!("Clipboard copy failed: {}", e);
    }
    result
}

#[cfg(target_os = "windows")]
fn copy_platform(text: &str) -> Result<(), String> {
    pipe_to_command("clip.exe", &[], text)
}

#[cfg(target_os = "macos")]
fn copy_platform(text: &str) -> Result<(), String> {
    pipe_to_command("pbcopy", &[], text)
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn copy_platform(text: &str) -> Result<(), String> {
    // Try Wayland first, then X11 clipboard utilities.
    if pipe_to_command("wl-copy", &[], text).is_ok() {
        return Ok(());
    }
    if pipe_to_command("xclip", &["-selection", "clipboard"], text).is_ok() {
        return Ok(());
    }
    pipe_to_command("xsel", &["--clipboard", "--input"], text)
}

fn pipe_to_command(cmd: &str, args: &[&str], input: &str) -> Result<(), String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{cmd}: {e}"))?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| format!("{cmd}: no stdin"))?
        .write_all(input.as_bytes())
        .map_err(|e| format!("{cmd}: write failed: {e}"))?;

    let status = child.wait().map_err(|e| format!("{cmd}: wait failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd}: exited with {status}"))
    }
}
