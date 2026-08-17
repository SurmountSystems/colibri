//! Tokio duplex session mapping subscribe interests to frame pushes.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};

use crate::error::{Error, Result};
use crate::stream::frame::{ClientFrame, PROTOCOL_VERSION, ServerFrame};
use crate::visual::Subscribe;

/// Async duplex session over any `AsyncRead + AsyncWrite` (or tokio duplex for tests).
pub struct DuplexSession<S> {
    pub(crate) stream: S,
    subscribe: Subscribe,
}

impl<S> DuplexSession<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    pub fn new(stream: S, subscribe: Subscribe) -> Self {
        Self { stream, subscribe }
    }

    pub fn subscribe(&self) -> Subscribe {
        self.subscribe
    }

    pub fn set_subscribe(&mut self, s: Subscribe) {
        self.subscribe = s;
    }

    /// Send a client frame.
    pub async fn send(&mut self, frame: &ClientFrame) -> Result<()> {
        let bytes = crate::stream::encode_frame(frame)?;
        self.stream
            .write_all(&bytes)
            .await
            .map_err(|e| Error::protocol(e.to_string()))?;
        self.stream
            .flush()
            .await
            .map_err(|e| Error::protocol(e.to_string()))?;
        Ok(())
    }

    /// Receive one server frame.
    pub async fn recv(&mut self) -> Result<ServerFrame> {
        let mut len_buf = [0u8; 4];
        self.stream
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| Error::protocol(e.to_string()))?;
        let len = u32::from_le_bytes(len_buf) as usize;
        if len > 64 * 1024 * 1024 {
            return Err(Error::protocol(format!("frame length too large: {len}")));
        }
        let mut body = vec![0u8; len];
        self.stream
            .read_exact(&mut body)
            .await
            .map_err(|e| Error::protocol(e.to_string()))?;
        let mut full = Vec::with_capacity(4 + len);
        full.extend_from_slice(&len_buf);
        full.extend_from_slice(&body);
        crate::stream::decode_frame(&full)
    }

    /// Convenience: send Hello-shaped server frame (host-side mock or gateway).
    pub async fn send_hello(
        &mut self,
        model_id: impl Into<String>,
        engine_name: impl Into<String>,
        kv_slots: u32,
    ) -> Result<()> {
        // Session is host-oriented for ClientFrame; for tests we still use
        // encode_frame on ServerFrame via raw write.
        let frame = ServerFrame::Hello {
            protocol_version: PROTOCOL_VERSION,
            model_id: model_id.into(),
            engine_name: engine_name.into(),
            kv_slots,
        };
        let bytes = crate::stream::encode_frame(&frame)?;
        self.stream
            .write_all(&bytes)
            .await
            .map_err(|e| Error::protocol(e.to_string()))?;
        self.stream
            .flush()
            .await
            .map_err(|e| Error::protocol(e.to_string()))?;
        Ok(())
    }
}

/// Create a pair of connected duplex streams for unit tests.
pub fn duplex_pair(max_buf: usize) -> (DuplexStream, DuplexStream) {
    tokio::io::duplex(max_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn duplex_ping_pong() {
        let (a, b) = duplex_pair(64 * 1024);
        let mut client = DuplexSession::new(a, Subscribe::ALL);
        let mut server = DuplexSession::new(b, Subscribe::ALL);

        let t = tokio::spawn(async move {
            // server reads client frame
            // For this test, server is the peer reading ClientFrame bytes as ServerFrame?
            // Better: both encode their own direction.
            let mut len = [0u8; 4];
            server.stream.read_exact(&mut len).await.unwrap();
            let n = u32::from_le_bytes(len) as usize;
            let mut body = vec![0u8; n];
            server.stream.read_exact(&mut body).await.unwrap();
            let mut full = Vec::new();
            full.extend_from_slice(&len);
            full.extend_from_slice(&body);
            let frame: ClientFrame = crate::stream::decode_frame(&full).unwrap();
            match frame {
                ClientFrame::Ping { nonce } => {
                    let pong = ServerFrame::Pong { nonce };
                    let bytes = crate::stream::encode_frame(&pong).unwrap();
                    server.stream.write_all(&bytes).await.unwrap();
                }
                _ => panic!("expected ping"),
            }
        });

        client.send(&ClientFrame::Ping { nonce: 99 }).await.unwrap();
        let resp = client.recv().await.unwrap();
        match resp {
            ServerFrame::Pong { nonce } => assert_eq!(nonce, 99),
            _ => panic!("expected pong"),
        }
        t.await.unwrap();
    }
}
