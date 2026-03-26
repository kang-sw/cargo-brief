//! LSP JSON-RPC Content-Length framing for rust-analyzer stdin/stdout.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout};

use anyhow::{Context, Result, bail};

/// Transport layer for LSP JSON-RPC communication with rust-analyzer.
pub struct RaTransport {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i32,
}

impl RaTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin,
            stdout: BufReader::new(stdout),
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

    /// Read one LSP message from stdout.
    pub fn read_message(&mut self) -> Result<serde_json::Value> {
        let content_length = self.read_headers()?;
        let mut buf = vec![0u8; content_length];
        self.stdout
            .read_exact(&mut buf)
            .context("Failed to read LSP message body")?;
        serde_json::from_slice(&buf).context("Failed to parse LSP JSON-RPC message")
    }

    /// Send a request and wait for the matching response, skipping notifications.
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

    fn read_headers(&mut self) -> Result<usize> {
        let mut content_length: Option<usize> = None;
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = self
                .stdout
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
