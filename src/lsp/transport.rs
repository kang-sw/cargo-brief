//! LSP JSON-RPC Content-Length framing for rust-analyzer stdin/stdout.

use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::{Context, Result, bail};

/// Transport layer for LSP JSON-RPC communication with rust-analyzer.
pub struct RaTransport {
    stdin: ChildStdin,
    stdout: Option<BufReader<ChildStdout>>,
    ra_rx: Option<Receiver<Result<serde_json::Value>>>,
    next_id: i32,
}

impl RaTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin,
            stdout: Some(BufReader::new(stdout)),
            ra_rx: None,
            next_id: 1,
        }
    }

    /// Send a JSON-RPC request. Returns the request ID.
    pub fn send_request(&mut self, method: &str, params: serde_json::Value) -> Result<i32> {
        let id = self.next_id;
        self.next_id += 1;

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        self.write_lsp_message(&msg)?;
        Ok(id)
    }

    /// Send a JSON-RPC notification (no id, no response expected).
    pub fn send_notification(&mut self, method: &str, params: serde_json::Value) -> Result<()> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        self.write_lsp_message(&msg)
    }

    /// Read one LSP message from stdout (direct) or channel (after thread spawn).
    pub fn read_message(&mut self) -> Result<serde_json::Value> {
        if let Some(stdout) = &mut self.stdout {
            Self::read_one_message(stdout)
        } else if let Some(rx) = &self.ra_rx {
            match rx.recv() {
                Ok(result) => result,
                Err(_) => bail!("rust-analyzer stdout closed"),
            }
        } else {
            bail!("transport has no reader")
        }
    }

    /// Spawn a background thread to read LSP messages from ra stdout.
    /// After this, read_message() reads from the channel (blocking).
    /// The thread exits when ra closes stdout or the receiver is dropped.
    pub fn spawn_reader_thread(&mut self) {
        let mut stdout = self.stdout.take().expect("reader thread already spawned");
        let (tx, rx) = mpsc::channel();
        self.ra_rx = Some(rx);
        std::thread::spawn(move || {
            loop {
                match Self::read_one_message(&mut stdout) {
                    Ok(msg) => {
                        if tx.send(Ok(msg)).is_err() {
                            break; // receiver dropped
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });
    }

    /// Non-blocking read. Returns Ok(Some(msg)) if available, Ok(None) if empty,
    /// or the error from the reader thread.
    /// Only available after spawn_reader_thread().
    pub fn try_read_message(&self) -> Result<Option<serde_json::Value>> {
        let rx = self.ra_rx.as_ref().expect("reader thread not spawned");
        match rx.try_recv() {
            Ok(result) => result.map(Some),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => bail!("rust-analyzer stdout closed"),
        }
    }

    /// Read with timeout. Returns Ok(Some(msg)) if available, Ok(None) on timeout,
    /// or the error from the reader thread.
    /// Only available after spawn_reader_thread().
    pub fn read_message_timeout(&self, timeout: Duration) -> Result<Option<serde_json::Value>> {
        let rx = self.ra_rx.as_ref().expect("reader thread not spawned");
        match rx.recv_timeout(timeout) {
            Ok(result) => result.map(Some),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => bail!("rust-analyzer stdout closed"),
        }
    }

    /// Send an LSP response to a server-initiated request (e.g.
    /// window/workDoneProgress/create). We are responding to a request FROM
    /// the server.
    pub fn send_raw_response(
        &mut self,
        id: serde_json::Value,
        result: serde_json::Value,
    ) -> Result<()> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        self.write_lsp_message(&msg)
    }

    /// Send a request and wait for the matching response, skipping notifications.
    /// Replies to server-initiated requests (e.g. window/workDoneProgress/create).
    /// Gives up after reading 10,000 messages without a matching response.
    pub fn send_request_and_wait(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let id = self.send_request(method, params)?;

        for _ in 0..10_000 {
            let msg = self.read_message()?;

            // Notifications have no "id" field — skip them
            if msg.get("id").is_none() {
                continue;
            }

            // Server-initiated request (has both "id" and "method") — reply and continue
            if msg.get("method").is_some() {
                if let Some(req_id) = msg.get("id").cloned() {
                    let _ = self.send_raw_response(req_id, serde_json::json!(null));
                }
                continue;
            }

            // Check if this is our response
            if msg["id"].as_i64() == Some(id as i64) {
                if let Some(error) = msg.get("error") {
                    bail!(
                        "LSP error on {method}: {} (code {})",
                        error["message"].as_str().unwrap_or("unknown"),
                        error["code"].as_i64().unwrap_or(-1)
                    );
                }
                return Ok(msg);
            }
        }

        bail!("Timed out waiting for LSP response to {method} (id={id})")
    }

    /// Read one complete LSP message from a buffered reader.
    /// Used by both direct reads (before thread spawn) and the background reader thread.
    fn read_one_message(reader: &mut impl BufRead) -> Result<serde_json::Value> {
        let content_length = Self::read_headers(reader)?;
        let mut buf = vec![0u8; content_length];
        reader
            .read_exact(&mut buf)
            .context("Failed to read LSP message body")?;
        serde_json::from_slice(&buf).context("Failed to parse LSP JSON-RPC message")
    }

    fn write_lsp_message(&mut self, msg: &serde_json::Value) -> Result<()> {
        let body = serde_json::to_string(msg).context("Failed to serialize LSP message")?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin
            .write_all(header.as_bytes())
            .context("Failed to write LSP header")?;
        self.stdin
            .write_all(body.as_bytes())
            .context("Failed to write LSP body")?;
        self.stdin.flush().context("Failed to flush to ra stdin")?;
        Ok(())
    }

    fn read_headers(reader: &mut impl BufRead) -> Result<usize> {
        let mut content_length: Option<usize> = None;
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader
                .read_line(&mut line)
                .context("Failed to read LSP header line")?;

            if bytes_read == 0 {
                bail!("rust-analyzer closed stdout unexpectedly");
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                // End of headers
                break;
            }

            if let Some(value) = trimmed.strip_prefix("Content-Length: ") {
                content_length = Some(
                    value
                        .parse()
                        .with_context(|| format!("Invalid Content-Length: {value}"))?,
                );
            }
            // Ignore other headers (Content-Type, etc.)
        }

        content_length.context("Missing Content-Length header in LSP message")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn content_length_format() {
        // Verify the format we produce matches LSP spec
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{}}"#;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        assert!(header.starts_with("Content-Length: "));
        assert!(header.ends_with("\r\n\r\n"));
    }
}
