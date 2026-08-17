//! Duplex rkyv streaming session.
//!
//! Envelope: little-endian `u32` length prefix + rkyv-archived body.
//! Frame taxonomy follows the plan / explore-visual-telemetry report.

#[cfg(feature = "stream")]
mod codec;
#[cfg(feature = "stream")]
mod frame;

#[cfg(feature = "stream")]
pub use codec::{
    decode_frame, decode_frame_checked, decode_server_frame_checked, encode_frame, read_frame,
    read_frame_with, write_frame,
};
#[cfg(feature = "stream")]
pub use frame::{ClientFrame, PROTOCOL_VERSION, ServerFrame};

#[cfg(all(feature = "stream", feature = "tokio"))]
mod session;

#[cfg(all(feature = "stream", feature = "tokio"))]
pub use session::{DuplexSession, duplex_pair};
