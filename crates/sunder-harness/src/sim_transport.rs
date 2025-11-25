//! Turmoil-based Transport implementation using TCP streams.

use std::{io, net::SocketAddr};

use async_trait::async_trait;
use sunder_core::transport::Transport;
use turmoil::net::{TcpListener, TcpStream};

/// Simulation transport using Turmoil's deterministic TCP streams.
///
/// This transport provides:
///
/// - **Deterministic delivery**: Turmoil controls packet ordering and timing
/// - **Fault injection**: Can simulate packet loss, delays, and partitions
/// - **Stream semantics**: Reliable, ordered byte delivery (like QUIC streams)
///
/// # Why TCP Instead of QUIC?
///
/// We don't simulate QUIC directly because:
///
/// 1. Quinn doesn't support pluggable time/RNG (would require forking)
/// 2. Sunder's protocol logic lives *inside* QUIC streams
/// 3. TCP provides identical stream guarantees for testing protocol correctness
///
/// We're testing Sunder's state machine, not QUIC's implementation.
///
/// # Usage
///
/// ```rust,ignore
/// use sunder_harness::SimTransport;
/// use sunder_core::transport::Transport;
///
/// // Server
/// turmoil::Builder::new().build().host("server", || async {
///     let transport = SimTransport::bind("0.0.0.0:443").await?;
///     let (mut send, mut recv) = transport.accept().await?;
///
///     // Read/write frames...
///     Ok(())
/// });
/// ```
pub struct SimTransport {
    listener: TcpListener,
}

impl SimTransport {
    /// Binds to the specified address for accepting connections.
    ///
    /// # Parameters
    ///
    /// - `addr`: Address to bind to (e.g., `"0.0.0.0:443"`)
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - The address is already in use
    /// - The address format is invalid
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let transport = SimTransport::bind("0.0.0.0:443").await?;
    /// ```
    pub async fn bind(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        Ok(Self { listener })
    }

    /// Connects to a remote address.
    ///
    /// # Parameters
    ///
    /// - `addr`: Remote address (e.g., `"server:443"`)
    ///
    /// # Returns
    ///
    /// Returns `io::Result<TcpStream>` — on success returns a connected
    /// `TcpStream`.
    ///
    /// The caller should use `tokio::io::split()` to separate the stream into
    /// read and write halves if needed.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - The remote host is unreachable
    /// - Connection is refused
    /// - DNS resolution fails
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let stream = SimTransport::connect_to("server:443").await?;
    /// let (recv, send) = tokio::io::split(stream);
    /// ```
    pub async fn connect_to(addr: &str) -> io::Result<TcpStream> {
        TcpStream::connect(addr).await
    }
}

use tokio::io::{ReadHalf, WriteHalf};

#[async_trait]
impl Transport for SimTransport {
    type SendStream = WriteHalf<TcpStream>;
    type RecvStream = ReadHalf<TcpStream>;

    async fn accept(&self) -> io::Result<(Self::SendStream, Self::RecvStream)> {
        let (stream, _addr) = self.listener.accept().await?;

        // Split the bidirectional stream into read and write halves
        let (recv, send) = tokio::io::split(stream);

        Ok((send, recv))
    }

    async fn connect(&self, addr: SocketAddr) -> io::Result<(Self::SendStream, Self::RecvStream)> {
        let stream = TcpStream::connect(addr).await?;

        // Split the bidirectional stream into read and write halves
        let (recv, send) = tokio::io::split(stream);

        Ok((send, recv))
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[test]
    fn sim_transport_echo() {
        let mut sim = turmoil::Builder::new().build();

        // Server: echo back whatever is received
        sim.host("server", || async {
            let transport = SimTransport::bind("0.0.0.0:443").await?;
            let (mut send, mut recv) = transport.accept().await?;

            let mut buf = [0u8; 128];
            let n = recv.read(&mut buf).await?;

            send.write_all(&buf[..n]).await?;

            Ok(())
        });

        // Client: send message, verify echo
        sim.client("client", async {
            let stream = SimTransport::connect_to("server:443").await?;
            let (mut recv, mut send) = tokio::io::split(stream);

            let message = b"Hello, Sunder!";
            send.write_all(message).await?;

            let mut buf = vec![0u8; message.len()];
            recv.read_exact(&mut buf).await?;

            assert_eq!(&buf, message);

            Ok(())
        });

        sim.run().expect("simulation failed");
    }

    #[test]
    fn sim_transport_bidirectional() {
        let mut sim = turmoil::Builder::new().build();

        // Server: send "pong" when receiving "ping"
        sim.host("server", || async {
            let transport = SimTransport::bind("0.0.0.0:443").await?;
            let (mut send, mut recv) = transport.accept().await?;

            let mut buf = [0u8; 4];
            recv.read_exact(&mut buf).await?;
            assert_eq!(&buf, b"ping");

            send.write_all(b"pong").await?;

            Ok(())
        });

        // Client: send "ping", expect "pong"
        sim.client("client", async {
            let stream = SimTransport::connect_to("server:443").await?;
            let (mut recv, mut send) = tokio::io::split(stream);

            send.write_all(b"ping").await?;

            let mut buf = [0u8; 4];
            recv.read_exact(&mut buf).await?;
            assert_eq!(&buf, b"pong");

            Ok(())
        });

        sim.run().expect("simulation failed");
    }
}
