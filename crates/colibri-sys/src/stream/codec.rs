//! Length-prefix + rkyv archive codec for stream frames.
//!
//! # Trust model
//!
//! | API | Validation | Use when |
//! |-----|------------|----------|
//! | [`decode_frame`] | **Trusted** `access_unchecked` | Local duplex pipes, same-process mock peers, already-authenticated hosts |
//! | [`decode_frame_checked`] | **bytecheck** via `rkyv::from_bytes` | Untrusted network / file / IPC buffers |
//!
//! Length-prefix framing is validated on both paths (too short / truncated /
//! oversized). Checked decode fails closed on corrupt rkyv bodies (no UB).

use std::io::{Read, Write};

use rkyv::rancor::Error as RkyvError;
use rkyv::util::AlignedVec;
use rkyv::{Archive, Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::stream::frame::{ClientFrame, ServerFrame};

/// Encode any serializable frame as `u32le length || rkyv body`.
pub fn encode_frame<T>(value: &T) -> Result<Vec<u8>>
where
    T: for<'a> Serialize<
        rkyv::api::high::HighSerializer<
            rkyv::util::AlignedVec,
            rkyv::ser::allocator::ArenaHandle<'a>,
            RkyvError,
        >,
    >,
{
    let body = rkyv::to_bytes::<RkyvError>(value)
        .map_err(|e| Error::protocol(format!("rkyv serialize: {e}")))?;
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

fn body_of(buf: &[u8]) -> Result<&[u8]> {
    if buf.len() < 4 {
        return Err(Error::protocol("frame too short for length prefix"));
    }
    let len = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
    if buf.len() < 4 + len {
        return Err(Error::protocol("frame truncated"));
    }
    Ok(&buf[4..4 + len])
}

fn align_body(body: &[u8]) -> AlignedVec<16> {
    // Copy into AlignedVec: length-prefix makes the body start at offset 4,
    // which is often under-aligned for rkyv's default alignment.
    let mut aligned = AlignedVec::<16>::new();
    aligned.extend_from_slice(body);
    aligned
}

/// Decode a length-prefixed buffer into owned `T` (trusted local pipes).
///
/// Prefer [`decode_frame_checked`] for untrusted transports.
pub fn decode_frame<T>(buf: &[u8]) -> Result<T>
where
    T: Archive,
    T::Archived: for<'a> Deserialize<T, rkyv::api::high::HighDeserializer<RkyvError>>,
{
    let body = body_of(buf)?;
    let aligned = align_body(body);
    // SAFETY: frames are produced by encode_frame on the same process or a
    // trusted peer. For untrusted links use decode_frame_checked.
    let archived = unsafe { rkyv::access_unchecked::<T::Archived>(aligned.as_slice()) };
    rkyv::deserialize::<T, RkyvError>(archived)
        .map_err(|e| Error::protocol(format!("rkyv deserialize: {e}")))
}

/// Decode a length-prefixed buffer with **bytecheck** validation.
///
/// Fails closed on truncated, misaligned, or corrupt rkyv archives (returns
/// [`Error::Protocol`], never UB). Use this for network / untrusted IPC.
pub fn decode_frame_checked<T>(buf: &[u8]) -> Result<T>
where
    T: Archive,
    T::Archived: for<'a> rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, RkyvError>>
        + Deserialize<T, rkyv::api::high::HighDeserializer<RkyvError>>,
{
    let body = body_of(buf)?;
    let aligned = align_body(body);
    rkyv::from_bytes::<T, RkyvError>(aligned.as_slice())
        .map_err(|e| Error::protocol(format!("rkyv validated decode: {e}")))
}

/// Decode a length-prefixed client frame (trusted).
#[allow(dead_code)]
pub fn decode_client_frame(buf: &[u8]) -> Result<ClientFrame> {
    decode_frame(buf)
}

/// Decode a length-prefixed server frame (trusted).
pub fn decode_server_frame(buf: &[u8]) -> Result<ServerFrame> {
    decode_frame(buf)
}

/// Decode a length-prefixed server frame with bytecheck.
pub fn decode_server_frame_checked(buf: &[u8]) -> Result<ServerFrame> {
    decode_frame_checked(buf)
}

/// Write one frame to a writer.
pub fn write_frame<W: Write, T>(w: &mut W, value: &T) -> Result<()>
where
    T: for<'a> Serialize<
        rkyv::api::high::HighSerializer<
            rkyv::util::AlignedVec,
            rkyv::ser::allocator::ArenaHandle<'a>,
            RkyvError,
        >,
    >,
{
    let bytes = encode_frame(value)?;
    w.write_all(&bytes)?;
    w.flush()?;
    Ok(())
}

/// Read one length-prefixed server frame from a reader (trusted decode).
pub fn read_frame<R: Read>(r: &mut R) -> Result<ServerFrame> {
    read_frame_with(r, false)
}

/// Read one length-prefixed server frame; `checked` enables bytecheck.
pub fn read_frame_with<R: Read>(r: &mut R, checked: bool) -> Result<ServerFrame> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 64 * 1024 * 1024 {
        return Err(Error::protocol(format!("frame length too large: {len}")));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    let mut full = Vec::with_capacity(4 + len);
    full.extend_from_slice(&len_buf);
    full.extend_from_slice(&body);
    if checked {
        decode_server_frame_checked(&full)
    } else {
        decode_server_frame(&full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::frame::{ClientFrame, PROTOCOL_VERSION, ServerFrame};

    #[test]
    fn client_frame_roundtrip() {
        let f = ClientFrame::Submit {
            req_id: 42,
            slot: 0,
            max_tokens: 64,
            temperature: 0.8,
            top_p: 0.95,
            prompt: "hello".into(),
            grammar: Some("root ::= \"ok\"".into()),
        };
        let bytes = encode_frame(&f).unwrap();
        let back: ClientFrame = decode_frame(&bytes).unwrap();
        match back {
            ClientFrame::Submit {
                req_id,
                prompt,
                grammar,
                max_tokens,
                temperature,
                slot,
                ..
            } => {
                assert_eq!(req_id, 42);
                assert_eq!(prompt, "hello");
                assert_eq!(grammar.as_deref(), Some("root ::= \"ok\""));
                assert_eq!(max_tokens, 64);
                assert!((temperature - 0.8).abs() < 1e-6);
                assert_eq!(slot, 0);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn server_expert_map_roundtrip() {
        let f = ServerFrame::ExpertMap {
            rows: 2,
            cols: 4,
            cells: vec![0x41, 0x82, 0x03, 0x00, 0x10, 0x20, 0x30, 0x40],
        };
        let bytes = encode_frame(&f).unwrap();
        let back: ServerFrame = decode_frame(&bytes).unwrap();
        match back {
            ServerFrame::ExpertMap { rows, cols, cells } => {
                assert_eq!(rows, 2);
                assert_eq!(cols, 4);
                assert_eq!(cells.len(), 8);
                assert_eq!(cells[0], 0x41);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn hello_protocol_version() {
        let f = ServerFrame::Hello {
            protocol_version: PROTOCOL_VERSION,
            model_id: "m".into(),
            engine_name: "colibri".into(),
            kv_slots: 1,
        };
        let bytes = encode_frame(&f).unwrap();
        let back: ServerFrame = decode_frame(&bytes).unwrap();
        match back {
            ServerFrame::Hello {
                protocol_version, ..
            } => assert_eq!(protocol_version, PROTOCOL_VERSION),
            _ => panic!("wrong"),
        }
    }

    #[test]
    fn checked_decode_accepts_valid_frame() {
        let f = ServerFrame::Pong { nonce: 7 };
        let bytes = encode_frame(&f).unwrap();
        let back: ServerFrame = decode_frame_checked(&bytes).unwrap();
        match back {
            ServerFrame::Pong { nonce } => assert_eq!(nonce, 7),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn checked_decode_rejects_garbage_body() {
        // Valid length prefix, nonsense body → fail closed (no panic / UB).
        let mut buf = Vec::new();
        let garbage = [0u8; 32];
        buf.extend_from_slice(&(garbage.len() as u32).to_le_bytes());
        buf.extend_from_slice(&garbage);
        let err = decode_frame_checked::<ServerFrame>(&buf).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("validated decode") || msg.contains("rkyv") || msg.contains("protocol"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn checked_decode_rejects_truncated() {
        let f = ClientFrame::Ping { nonce: 1 };
        let bytes = encode_frame(&f).unwrap();
        let err = decode_frame_checked::<ClientFrame>(&bytes[..bytes.len().saturating_sub(3)])
            .unwrap_err();
        assert!(err.to_string().contains("truncated") || err.to_string().contains("too short"));
    }

    #[test]
    fn checked_decode_rejects_corrupt_length() {
        // Claim huge body but only a few bytes present.
        let buf = [0xff, 0xff, 0x00, 0x00, 1, 2, 3];
        let err = decode_frame_checked::<ServerFrame>(&buf).unwrap_err();
        assert!(err.to_string().contains("truncated"));
    }
}
