//! UDS message types for client ↔ daemon communication.
//!
//! Framing: 4-byte little-endian length prefix + JSON payload.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Serialize, Deserialize, Debug)]
pub enum DaemonRequest {
    Ping,
    Stop,
    Status,
    References {
        symbol: String,
        quiet: bool,
    },
    BlastRadius {
        symbol: String,
        depth: u32,
        quiet: bool,
    },
    CallHierarchy {
        symbol: String,
        outgoing: bool,
        quiet: bool,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum DaemonResponse {
    Ok {
        message: String,
    },
    Status {
        pid: u32,
        ra_status: RaStatus,
        uptime_secs: u64,
    },
    Error {
        message: String,
    },
    QueryResult {
        output: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaStatus {
    Initializing,
    Ready,
    Stopped,
}

impl std::fmt::Display for RaStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RaStatus::Initializing => write!(f, "initializing"),
            RaStatus::Ready => write!(f, "ready"),
            RaStatus::Stopped => write!(f, "stopped"),
        }
    }
}

/// Write a length-prefixed JSON message to a UDS stream.
pub fn write_message(stream: &mut UnixStream, msg: &impl Serialize) -> Result<()> {
    let json = serde_json::to_vec(msg).context("Failed to serialize message")?;
    let len = json.len() as u32;
    stream
        .write_all(&len.to_le_bytes())
        .context("Failed to write message length")?;
    stream
        .write_all(&json)
        .context("Failed to write message body")?;
    stream.flush().context("Failed to flush stream")?;
    Ok(())
}

/// Read a length-prefixed JSON message from a UDS stream.
pub fn read_message<T: DeserializeOwned>(stream: &mut UnixStream) -> Result<T> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .context("Failed to read message length")?;
    let len = u32::from_le_bytes(len_buf) as usize;

    if len > 1024 * 1024 {
        bail!("Message too large: {len} bytes");
    }

    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .context("Failed to read message body")?;
    serde_json::from_slice(&buf).context("Failed to deserialize message")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn roundtrip_request() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let req = DaemonRequest::Ping;
        write_message(&mut a, &req).unwrap();
        let got: DaemonRequest = read_message(&mut b).unwrap();
        assert!(matches!(got, DaemonRequest::Ping));
    }

    #[test]
    fn roundtrip_response() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let resp = DaemonResponse::Status {
            pid: 42,
            ra_status: RaStatus::Ready,
            uptime_secs: 120,
        };
        write_message(&mut a, &resp).unwrap();
        let got: DaemonResponse = read_message(&mut b).unwrap();
        match got {
            DaemonResponse::Status {
                pid,
                ra_status,
                uptime_secs,
            } => {
                assert_eq!(pid, 42);
                assert_eq!(ra_status, RaStatus::Ready);
                assert_eq!(uptime_secs, 120);
            }
            _ => panic!("unexpected response variant"),
        }
    }

    #[test]
    fn roundtrip_references_request() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let req = DaemonRequest::References {
            symbol: "Foo::bar".to_string(),
            quiet: true,
        };
        write_message(&mut a, &req).unwrap();
        let got: DaemonRequest = read_message(&mut b).unwrap();
        match got {
            DaemonRequest::References { symbol, quiet } => {
                assert_eq!(symbol, "Foo::bar");
                assert!(quiet);
            }
            _ => panic!("unexpected request variant"),
        }
    }

    #[test]
    fn roundtrip_blast_radius_request() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let req = DaemonRequest::BlastRadius {
            symbol: "resolve_symbol".to_string(),
            depth: 3,
            quiet: false,
        };
        write_message(&mut a, &req).unwrap();
        let got: DaemonRequest = read_message(&mut b).unwrap();
        match got {
            DaemonRequest::BlastRadius {
                symbol,
                depth,
                quiet,
            } => {
                assert_eq!(symbol, "resolve_symbol");
                assert_eq!(depth, 3);
                assert!(!quiet);
            }
            _ => panic!("unexpected request variant"),
        }
    }

    #[test]
    fn roundtrip_call_hierarchy_request() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let req = DaemonRequest::CallHierarchy {
            symbol: "Foo::bar".to_string(),
            outgoing: true,
            quiet: true,
        };
        write_message(&mut a, &req).unwrap();
        let got: DaemonRequest = read_message(&mut b).unwrap();
        match got {
            DaemonRequest::CallHierarchy {
                symbol,
                outgoing,
                quiet,
            } => {
                assert_eq!(symbol, "Foo::bar");
                assert!(outgoing);
                assert!(quiet);
            }
            _ => panic!("unexpected request variant"),
        }
    }

    #[test]
    fn roundtrip_query_result_response() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let resp = DaemonResponse::QueryResult {
            output: "// 2 references to Foo\n".to_string(),
        };
        write_message(&mut a, &resp).unwrap();
        let got: DaemonResponse = read_message(&mut b).unwrap();
        match got {
            DaemonResponse::QueryResult { output } => {
                assert_eq!(output, "// 2 references to Foo\n");
            }
            _ => panic!("unexpected response variant"),
        }
    }
}
