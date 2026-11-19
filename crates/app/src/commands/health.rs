use lazypock_core::{Config, Error, Result};
use std::time::Duration;

/// Timeout for a single `healthcheck` probe.
const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn healthcheck(config: &Config) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = config.client_addr();

    let probe = async {
        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .map_err(|err| Error::Internal(format!("cannot connect to {addr}: {err}")))?;

        let request =
            format!("GET /api/health HTTP/1.0\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|err| Error::Internal(format!("failed to send probe: {err}")))?;
        stream
            .flush()
            .await
            .map_err(|err| Error::Internal(format!("failed to flush probe: {err}")))?;

        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .map_err(|err| Error::Internal(format!("failed to read probe response: {err}")))?;

        Ok::<String, Error>(String::from_utf8_lossy(&response).into_owned())
    };

    let response = tokio::time::timeout(HEALTHCHECK_TIMEOUT, probe)
        .await
        .map_err(|_| Error::Internal(format!("healthcheck against {addr} timed out")))??;

    let status_line = response.lines().next().unwrap_or_default();
    if status_line.contains(" 200") {
        println!("ok");
        Ok(())
    } else {
        Err(Error::Internal(format!(
            "unhealthy response from {addr}: {status_line}"
        )))
    }
}
