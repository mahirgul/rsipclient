//! IPC client - sends commands to the running service via TCP

use crate::ipc::{Request, Response};
use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Send a JSON request to the running service and get a response.
///
/// Connects to `127.0.0.1:{ctrl_port}`, sends one JSON line, reads response.
pub async fn send_ipc(req: &Request, ctrl_port: u16) -> Result<Response> {
    let addr = format!("127.0.0.1:{}", ctrl_port);
    let stream = TcpStream::connect(&addr).await.context(format!(
        "Cannot connect to service on {}. Is the service running? (start with: sip-client service)",
        addr
    ))?;

    let (reader, mut writer) = stream.into_split();

    // Send JSON request line
    let json = serde_json::to_string(req)?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.shutdown().await?; // signal end of write

    // Read JSON response line
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader.read_line(&mut line).await?;

    let resp: Response =
        serde_json::from_str(line.trim()).context("Failed to parse service response")?;

    Ok(resp)
}
