use std::path::Path;

use tokio::net::UnixDatagram;

/// Listen on a Unix datagram socket for sd_notify READY=1 messages.
/// Returns when the process sends READY=1 or the timeout expires.
pub async fn wait_for_ready(socket_path: &Path, timeout: std::time::Duration) -> crate::Result<()> {
    if socket_path.exists() {
        let _ = tokio::fs::remove_file(socket_path).await;
    }

    let socket = UnixDatagram::bind(socket_path).map_err(|e| {
        crate::SupervisorError::Other(anyhow::anyhow!(
            "failed to bind notify socket at {}: {e}",
            socket_path.display()
        ))
    })?;

    let mut buf = [0u8; 4096];

    let result = tokio::time::timeout(timeout, async {
        loop {
            let n = socket.recv(&mut buf).await.map_err(|e| {
                crate::SupervisorError::Other(anyhow::anyhow!("notify socket recv: {e}"))
            })?;

            let msg = std::str::from_utf8(&buf[..n]).unwrap_or("");

            // sd_notify messages are newline-separated key=value pairs.
            for line in msg.split('\n') {
                if line.trim() == "READY=1" {
                    return Ok(());
                }
            }
        }
    })
    .await;

    // Clean up the socket file.
    let _ = tokio::fs::remove_file(socket_path).await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(crate::SupervisorError::ProbeTimeout(
            socket_path.display().to_string(),
        )),
    }
}
