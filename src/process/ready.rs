use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, Notify};

#[derive(Debug, Clone, PartialEq)]
pub enum ReadyResult {
    Ready,
    TimedOut,
    Failed,
}

#[derive(Debug, Clone)]
pub enum CheckType {
    SmartDefault,
    Exit,
    Tcp(u16),
    Http { url: String, status: Option<u16> },
    LogLine,
}

/// Smart default: if the process exits quickly with code 0, it's a one-shot
/// task. If it stays alive past the grace period, it's a long-running service.
pub async fn wait_smart_default(mut exit_rx: watch::Receiver<Option<Option<i32>>>) -> ReadyResult {
    let grace_period = Duration::from_secs(2);

    // Mark current value as seen so changed() only fires on new sends
    exit_rx.borrow_and_update();

    match tokio::time::timeout(grace_period, exit_rx.changed()).await {
        Ok(Ok(())) => {
            let exit_code = *exit_rx.borrow();
            match exit_code {
                Some(Some(0)) => ReadyResult::Ready,
                Some(_) => ReadyResult::Failed,
                None => ReadyResult::Ready,
            }
        }
        Err(_) => {
            // Grace period elapsed, process still running
            ReadyResult::Ready
        }
        Ok(Err(_)) => ReadyResult::Failed,
    }
}

/// Wait for the process to exit with code 0.
pub async fn wait_exit(
    mut exit_rx: watch::Receiver<Option<Option<i32>>>,
    timeout: Duration,
) -> ReadyResult {
    // Mark current value as seen so changed() only fires on new sends
    exit_rx.borrow_and_update();

    match tokio::time::timeout(timeout, exit_rx.changed()).await {
        Ok(Ok(())) => {
            let exit_code = *exit_rx.borrow();
            match exit_code {
                Some(Some(0)) => ReadyResult::Ready,
                Some(_) => ReadyResult::Failed,
                None => ReadyResult::Ready,
            }
        }
        Err(_) => ReadyResult::TimedOut,
        Ok(Err(_)) => ReadyResult::Failed,
    }
}

/// Wait for a TCP port to accept connections.
pub async fn wait_tcp(port: u16, timeout: Duration, interval: Duration) -> ReadyResult {
    let deadline = tokio::time::Instant::now() + timeout;
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();

    loop {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(_) => return ReadyResult::Ready,
            Err(_) => {
                if tokio::time::Instant::now() >= deadline {
                    return ReadyResult::TimedOut;
                }
                tokio::time::sleep(interval).await;
            }
        }
    }
}

/// Wait for an HTTP endpoint to return the expected status code.
/// Only supports plain HTTP (not HTTPS).
pub async fn wait_http(
    url: &str,
    expected_status: Option<u16>,
    timeout: Duration,
    interval: Duration,
) -> ReadyResult {
    let deadline = tokio::time::Instant::now() + timeout;
    let expected = expected_status.unwrap_or(200);

    loop {
        match simple_http_get(url).await {
            Ok(status_code) if status_code == expected => return ReadyResult::Ready,
            _ => {
                if tokio::time::Instant::now() >= deadline {
                    return ReadyResult::TimedOut;
                }
                tokio::time::sleep(interval).await;
            }
        }
    }
}

/// Wait for a log line pattern to appear in stdout/stderr.
pub async fn wait_log_line(
    ready_flag: Arc<AtomicBool>,
    ready_notify: Arc<Notify>,
    timeout: Duration,
) -> ReadyResult {
    // Check if the pattern was already matched before we started waiting
    if ready_flag.load(Ordering::Acquire) {
        return ReadyResult::Ready;
    }

    match tokio::time::timeout(timeout, ready_notify.notified()).await {
        Ok(()) => ReadyResult::Ready,
        Err(_) => ReadyResult::TimedOut,
    }
}

/// Minimal HTTP/1.1 GET request using raw TCP.
/// Returns the HTTP status code on success.
async fn simple_http_get(url: &str) -> anyhow::Result<u16> {
    let url = url.strip_prefix("http://").unwrap_or(url);

    let (host_port, path) = match url.find('/') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, "/"),
    };

    let (host, port) = match host_port.rfind(':') {
        Some(i) => (&host_port[..i], host_port[i + 1..].parse::<u16>().unwrap_or(80)),
        None => (host_port, 80u16),
    };

    let addr: std::net::SocketAddr = (
        host.parse::<std::net::Ipv4Addr>()
            .unwrap_or(std::net::Ipv4Addr::LOCALHOST),
        port,
    )
        .into();

    let mut stream = tokio::net::TcpStream::connect(addr).await?;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let request = format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, host);
    stream.write_all(request.as_bytes()).await?;

    let mut buf = [0u8; 256];
    let n = stream.read(&mut buf).await?;
    let response = std::str::from_utf8(&buf[..n])?;

    // Parse status code from "HTTP/1.1 200 OK"
    let status_line = response.lines().next().unwrap_or("");
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| anyhow::anyhow!("Failed to parse HTTP status from: {}", status_line))?;

    Ok(status_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wait_smart_default_long_running() {
        let (_tx, rx) = watch::channel(None);
        // Process stays alive -> should be Ready after grace period
        let result = wait_smart_default(rx).await;
        assert_eq!(result, ReadyResult::Ready);
    }

    #[tokio::test]
    async fn test_wait_smart_default_exit_success() {
        let (tx, rx) = watch::channel(None);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = tx.send(Some(Some(0)));
        });
        let result = wait_smart_default(rx).await;
        assert_eq!(result, ReadyResult::Ready);
    }

    #[tokio::test]
    async fn test_wait_smart_default_exit_failure() {
        let (tx, rx) = watch::channel(None);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = tx.send(Some(Some(1)));
        });
        let result = wait_smart_default(rx).await;
        assert_eq!(result, ReadyResult::Failed);
    }

    #[tokio::test]
    async fn test_wait_exit_success() {
        let (tx, rx) = watch::channel(None);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = tx.send(Some(Some(0)));
        });
        let result = wait_exit(rx, Duration::from_secs(5)).await;
        assert_eq!(result, ReadyResult::Ready);
    }

    #[tokio::test]
    async fn test_wait_exit_timeout() {
        let (_tx, rx) = watch::channel(None);
        let result = wait_exit(rx, Duration::from_millis(200)).await;
        assert_eq!(result, ReadyResult::TimedOut);
    }

    #[tokio::test]
    async fn test_wait_exit_failure() {
        let (tx, rx) = watch::channel(None);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = tx.send(Some(Some(1)));
        });
        let result = wait_exit(rx, Duration::from_secs(5)).await;
        assert_eq!(result, ReadyResult::Failed);
    }

    #[tokio::test]
    async fn test_wait_tcp_success() {
        // Bind a TCP listener, then check that wait_tcp finds it
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let result = wait_tcp(port, Duration::from_secs(5), Duration::from_millis(100)).await;
        assert_eq!(result, ReadyResult::Ready);
    }

    #[tokio::test]
    async fn test_wait_tcp_timeout() {
        // Use a port that is almost certainly not listening
        let result = wait_tcp(19999, Duration::from_millis(500), Duration::from_millis(100)).await;
        assert_eq!(result, ReadyResult::TimedOut);
    }

    #[tokio::test]
    async fn test_wait_log_line_already_matched() {
        let flag = Arc::new(AtomicBool::new(true));
        let notify = Arc::new(Notify::new());
        let result = wait_log_line(flag, notify, Duration::from_secs(1)).await;
        assert_eq!(result, ReadyResult::Ready);
    }

    #[tokio::test]
    async fn test_wait_log_line_match_during_wait() {
        let flag = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let flag2 = flag.clone();
        let notify2 = notify.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            flag2.store(true, Ordering::Release);
            notify2.notify_one();
        });
        let result = wait_log_line(flag, notify, Duration::from_secs(5)).await;
        assert_eq!(result, ReadyResult::Ready);
    }

    #[tokio::test]
    async fn test_wait_log_line_timeout() {
        let flag = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let result = wait_log_line(flag, notify, Duration::from_millis(200)).await;
        assert_eq!(result, ReadyResult::TimedOut);
    }
}
