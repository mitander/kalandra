//! Transport abstraction for network I/O.
//!
//! The `Transport` trait abstracts over reliable, bidirectional byte streams.
//! This allows the same protocol logic to run over:
//!
//! - **QUIC streams** (production via Quinn)
//! - **TCP streams** (simulation via Turmoil)
//!
//! # Why Not Simulate QUIC Directly?
//!
//! Quinn does not support pluggable time/RNG providers, so deterministic
//! QUIC simulation would require forking Quinn indefinitely.
//!
//! Instead, we abstract at the **stream level**:
//!
//! - Sunder's protocol logic lives *inside* QUIC streams
//! - We test Sunder's correctness, not QUIC's reliability
//! - Turmoil's TCP provides identical stream semantics for testing
//!
//! # What We're NOT Testing
//!
//! - QUIC-specific behavior (connection migration, 0-RTT, loss recovery)
//! - UDP datagram delivery (we use reliable streams for control plane)
//! - Congestion control algorithms
//!
//! # What We ARE Testing
//!
//! - Sunder protocol state machine correctness
//! - Message ordering and sequencing
//! - Epoch transitions under network faults
//! - Timeout and retry logic
//!
//! # Example
//!
//! ```rust,ignore
//! use sunder_core::transport::Transport;
//!
//! async fn handle_connection<T: Transport>(transport: T) -> Result<()> {
//!     let (mut send, mut recv) = transport.accept().await?;
//!
//!     // Read frame header (128 bytes)
//!     let mut header_buf = [0u8; 128];
//!     recv.read_exact(&mut header_buf).await?;
//!
//!     // Process frame...
//!
//!     Ok(())
//! }
//! ```

use std::{io, net::SocketAddr};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

/// Abstract transport for reliable, ordered byte streams.
///
/// This trait represents a connection-oriented transport that provides
/// bidirectional byte streams with the following guarantees:
///
/// - **Reliability**: Bytes are delivered or an error is returned
/// - **Ordering**: Bytes arrive in the order they were sent
/// - **Flow Control**: Backpressure prevents sender from overwhelming receiver
///
/// # Implementations
///
/// - **`QuinnTransport`** (production): Uses QUIC bidirectional streams
/// - **`TurmoilTransport`** (simulation): Uses deterministic TCP streams
///
/// # Design Note: Why Bidirectional Streams?
///
/// QUIC supports unidirectional streams, but Sunder's protocol is
/// request-response oriented (client sends Frame, server responds with Frame).
/// Bidirectional streams simplify this pattern.
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Type of stream for sending data.
    ///
    /// Must implement `AsyncWrite` for writing bytes.
    type SendStream: AsyncWrite + Unpin + Send + 'static;

    /// Type of stream for receiving data.
    ///
    /// Must implement `AsyncRead` for reading bytes.
    type RecvStream: AsyncRead + Unpin + Send + 'static;

    /// Accepts an incoming connection, returning send/receive streams.
    ///
    /// # Behavior
    ///
    /// - **Blocks** until a connection is available
    /// - **Returns** a pair of streams: `(send, recv)`
    /// - **Errors** if the endpoint is closed or network fails
    ///
    /// # Implementation Notes
    ///
    /// - **Quinn**: Calls `endpoint.accept()` then `connection.accept_bi()`
    /// - **Turmoil**: Calls `listener.accept()` then splits the TCP stream
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if:
    /// - The endpoint is shut down
    /// - The connection fails during handshake
    /// - The peer closes the connection immediately
    async fn accept(&self) -> io::Result<(Self::SendStream, Self::RecvStream)>;

    /// Connects to a remote endpoint, returning send/receive streams.
    ///
    /// # Parameters
    ///
    /// - `addr`: Socket address of the remote endpoint
    ///
    /// # Behavior
    ///
    /// - **Initiates** a connection to the remote address
    /// - **Waits** for the handshake to complete
    /// - **Returns** a pair of streams: `(send, recv)`
    ///
    /// # Implementation Notes
    ///
    /// - **Quinn**: Calls `endpoint.connect(addr)` then `connection.open_bi()`
    /// - **Turmoil**: Calls `TcpStream::connect(addr)` then splits
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if:
    /// - The remote endpoint is unreachable
    /// - The handshake fails (TLS, QUIC)
    /// - The connection is refused
    async fn connect(&self, addr: SocketAddr) -> io::Result<(Self::SendStream, Self::RecvStream)>;
}

/// Extension trait for splitting bidirectional streams.
///
/// Some transport implementations (like Turmoil's TCP) use a single
/// bidirectional stream that needs to be "split" into separate read/write
/// halves. This trait provides that operation.
///
/// # Example
///
/// ```rust,ignore
/// let stream = TcpStream::connect(addr).await?;
/// let (read_half, write_half) = stream.split();
/// ```
pub trait SplittableStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {
    /// Type of the read half after splitting.
    type ReadHalf: AsyncRead + Unpin + Send + 'static;

    /// Type of the write half after splitting.
    type WriteHalf: AsyncWrite + Unpin + Send + 'static;

    /// Splits this stream into separate read and write halves.
    ///
    /// The split halves can be used concurrently (e.g., on different tasks).
    fn split(self) -> (Self::ReadHalf, Self::WriteHalf);
}
