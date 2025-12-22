//! Transport abstraction for network I/O.
//!
//! The `Transport` trait abstracts over connection-oriented transports that
//! support multiplexed streams. This matches the QUIC model:
//!
//! - **Connection**: Long-lived, can migrate between IPs, connection-level
//!   errors
//! - **Streams**: Short-lived, multiplexed over connection, cheap to create
//!
//! # Implementations
//!
//! - **`QuinnTransport`** (production): Uses QUIC connections and streams
//! - **`SimTransport`** (testing): Simulates QUIC semantics over Turmoil's TCP
//!
//! # Why Not Simulate QUIC Directly?
//!
//! Quinn does not support pluggable time/RNG providers, so deterministic
//! QUIC simulation would require forking Quinn indefinitely.
//!
//! Instead, we abstract at the **connection level** but simulate with TCP:
//!
//! - Lockframe's protocol logic lives inside QUIC streams
//! - We test Lockframe's correctness, not QUIC's reliability
//! - Turmoil's TCP provides identical stream semantics for testing
//!
//! # What We're NOT Testing
//!
//! - QUIC-specific behavior (0-RTT, loss recovery, congestion control)
//! - Connection migration between IPs
//! - UDP datagram delivery
//!
//! # What We ARE Testing
//!
//! - Lockframe protocol state machine correctness
//! - Message ordering and sequencing
//! - Timeout and retry logic
//! - Network fault handling

use std::{io, net::SocketAddr};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

/// Abstract transport for connection-oriented protocols with multiplexed
/// streams.
///
/// This trait models QUIC's architecture:
/// - One **Connection** can have many **Streams**
/// - Connections are long-lived and have connection-level operations
/// - Streams are cheap, multiplexed, and have stream-level operations
///
/// # Lifecycle
///
/// ```text
/// Server:                      Client:
/// Transport::bind()            Transport::connect()
///   ↓                            ↓
/// accept()                     [Connection returned]
///   ↓                            ↓
/// [Connection returned]        open_bi() / accept_bi()
///   ↓                            ↓
/// accept_bi()                  [Stream returned]
///   ↓
/// [Stream returned]
/// ```
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Type representing a connection to a peer.
    ///
    /// A connection is long-lived and supports:
    /// - Opening new streams
    /// - Accepting incoming streams
    /// - Connection-level close with error code
    type Connection: TransportConnection;

    /// Accept an incoming connection.
    ///
    /// # Behavior
    ///
    /// - **Blocks** until a connection is established
    /// - **Returns** a Connection handle
    /// - **Errors** if the endpoint is closed or handshake fails
    ///
    /// # Implementation Notes
    ///
    /// - **Quinn**: `endpoint.accept()` then complete handshake
    /// - **Turmoil**: `listener.accept()` returns TCP connection
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if:
    /// - The endpoint is shut down
    /// - The connection fails during handshake
    /// - Network errors occur
    async fn accept(&self) -> io::Result<Self::Connection>;

    /// Connect to a remote endpoint.
    ///
    /// # Behavior
    ///
    /// - **Initiates** a connection to the remote address
    /// - **Waits** for the handshake to complete
    /// - **Returns** a Connection handle
    ///
    /// # Implementation Notes
    ///
    /// - **Quinn**: `endpoint.connect(addr)` and await handshake
    /// - **Turmoil**: `TcpStream::connect(addr)`
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if:
    /// - The remote endpoint is unreachable
    /// - The handshake fails
    /// - The connection is refused
    async fn connect(&self, remote: SocketAddr) -> io::Result<Self::Connection>;
}

/// A connection to a remote peer, supporting multiplexed streams.
///
/// This trait represents a QUIC connection or its simulation equivalent.
/// Multiple streams can be opened/accepted concurrently over a single
/// connection.
#[async_trait]
pub trait TransportConnection: Send + Sync + 'static {
    /// Type of stream for sending data.
    type SendStream: AsyncWrite + Unpin + Send + 'static;

    /// Type of stream for receiving data.
    type RecvStream: AsyncRead + Unpin + Send + 'static;

    /// Open a new bidirectional stream.
    ///
    /// # Behavior
    ///
    /// - **Creates** a new stream over this connection
    /// - **Returns** send and receive halves
    /// - **Cheap**: Stream creation is lightweight (multiplexing)
    ///
    /// # Implementation Notes
    ///
    /// - **Quinn**: `connection.open_bi()`
    /// - **Turmoil**: Simulated (returns same underlying TCP, but logically
    ///   separate)
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if:
    /// - Connection is closed
    /// - Peer rejected the stream
    /// - Flow control limits exceeded
    async fn open_bi(&self) -> io::Result<(Self::SendStream, Self::RecvStream)>;

    /// Accept an incoming bidirectional stream.
    ///
    /// # Behavior
    ///
    /// - **Blocks** until peer opens a stream
    /// - **Returns** send and receive halves
    /// - **None** if connection is closed
    ///
    /// # Implementation Notes
    ///
    /// - **Quinn**: `connection.accept_bi()`
    /// - **Turmoil**: Simulated (waits for peer's open_bi)
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if network errors occur.
    /// Returns `Ok(None)` if connection is gracefully closed.
    async fn accept_bi(&self) -> io::Result<Option<(Self::SendStream, Self::RecvStream)>>;

    /// Close the connection immediately with an error code.
    ///
    /// # Behavior
    ///
    /// - **Terminates** all streams on this connection
    /// - **Sends** close frame to peer with error code
    /// - **Non-blocking**: Returns immediately
    ///
    /// # Implementation Notes
    ///
    /// - **Quinn**: `connection.close(error_code, reason)`
    /// - **Turmoil**: Closes TCP socket
    fn close(&self, error_code: u64, reason: &str);
}
