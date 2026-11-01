//! Bounded private IPC framing. This is deliberately separate from MCP JSON-RPC.
use std::io;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const IPC_MAJOR: u32 = 1;
pub(crate) const IPC_MINOR: u32 = 0;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Message {
    Hello {
        ipc_major: u32,
        ipc_minor: u32,
        bridge_version: String,
    },
    Welcome {
        ipc_major: u32,
        ipc_minor: u32,
        backend_instance_id: String,
        application_version: String,
    },
    Request {
        request_id: String,
        method: String,
        arguments: Value,
    },
    Response {
        request_id: String,
        result: Value,
    },
    Cancel {
        request_id: String,
    },
}

pub(crate) async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Message> {
    let length = reader.read_u32_le().await? as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid IPC frame length",
        ));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid IPC message"))
}

pub(crate) async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    message: &Message,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(message)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "IPC serialization failed"))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC result exceeds message limit",
        ));
    }
    writer.write_u32_le(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, duplex};

    #[tokio::test]
    async fn reads_fragmented_and_coalesced_messages() {
        let (mut input, mut output) = duplex(16);
        let sender = tokio::spawn(async move {
            let json = br#"{"kind":"cancel","request_id":"first"}"#;
            for byte in (json.len() as u32).to_le_bytes().iter().chain(json.iter()) {
                output.write_all(&[*byte]).await.unwrap();
            }
            write_frame(
                &mut output,
                &Message::Cancel {
                    request_id: "second".into(),
                },
            )
            .await
            .unwrap();
        });
        assert!(
            matches!(read_frame(&mut input).await.unwrap(), Message::Cancel { request_id } if request_id == "first")
        );
        assert!(
            matches!(read_frame(&mut input).await.unwrap(), Message::Cancel { request_id } if request_id == "second")
        );
        sender.await.unwrap();
        assert_eq!(
            read_frame(&mut input).await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn rejects_lengths_before_reading_payload() {
        for length in [0, MAX_FRAME_BYTES as u32 + 1, u32::MAX] {
            let bytes = length.to_le_bytes();
            assert_eq!(
                read_frame(&mut bytes.as_slice()).await.unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[tokio::test]
    async fn rejects_invalid_utf8_unknown_fields_and_truncated_payloads() {
        for payload in [
            b"\xff".as_slice(),
            br#"{"kind":"cancel","request_id":"x","extra":true}"#,
            b"not json",
        ] {
            let mut bytes = (payload.len() as u32).to_le_bytes().to_vec();
            bytes.extend_from_slice(payload);
            assert_eq!(
                read_frame(&mut bytes.as_slice()).await.unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        let mut truncated = 10u32.to_le_bytes().to_vec();
        truncated.extend_from_slice(b"{}");
        assert_eq!(
            read_frame(&mut truncated.as_slice())
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
