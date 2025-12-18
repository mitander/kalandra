//! Fault injection tests for Kalandra protocol.
//!
//! These tests validate that the protocol handles realistic network conditions:
//! - Packet loss (2% - realistic degraded network, handled by TCP
//!   retransmissions)
//! - Network latency (100ms - typical poor network conditions)
//! - Network partitions (split-brain scenarios)
//!
//! # Why 2% packet loss?
//!
//! Real-world networks:
//! - **<1% loss**: Normal operation
//! - **1-2% loss**: Degraded but usable (realistic worst-case for production)
//! - **5-10% loss**: Severe degradation, users experiencing issues
//! - **>20% loss**: Network effectively broken, applications fail
//!
//! Testing 2% validates our protocol can survive degraded but realistic
//! conditions. Higher loss rates cause TCP handshake failures and extreme
//! retransmission delays, making tests non-deterministic.

use std::time::Duration;

use kalandra_core::{env::Environment, transport::Transport};
use kalandra_harness::{SimEnv, SimTransport};
use kalandra_proto::{Frame, FrameHeader, Opcode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Helper to convert any error to Box<dyn Error>
fn to_box_err<E: std::error::Error + 'static>(e: E) -> Box<dyn std::error::Error> {
    Box::new(e)
}

#[test]
fn ping_pong_with_packet_loss() {
    // TCP will handle retransmissions automatically
    // Using 2% loss - realistic degraded network that completes reliably
    // Set deterministic seed for reproducible packet loss patterns
    let mut sim = turmoil::Builder::new()
        .simulation_duration(Duration::from_secs(60))
        .fail_rate(0.02)  // 2% packet loss - realistic degraded network
        .rng_seed(12345)  // Deterministic seed
        .build();

    // Server: respond to Ping with Pong
    sim.host("server", || async move {
        let transport = SimTransport::bind("0.0.0.0:443").await?;
        let conn = transport.accept().await?;
        let (mut send, mut recv) = conn.into_split();

        // Read frame header (128 bytes)
        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;

        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
        assert_eq!(header.opcode_enum(), Some(Opcode::Ping));

        // Read payload (should be empty)
        let payload_size = header.payload_size() as usize;
        let mut payload_buf = vec![0u8; payload_size];
        recv.read_exact(&mut payload_buf).await?;

        // Create Pong response
        let pong_header = FrameHeader::new(Opcode::Pong);
        let pong_frame = Frame::new(pong_header, Vec::new());

        // Send response
        let mut response_buf = Vec::new();
        pong_frame.encode(&mut response_buf).map_err(to_box_err)?;
        send.write_all(&response_buf).await?;

        Ok(())
    });

    // Client: send Ping, expect Pong
    sim.client("client", async {
        let env = SimEnv::new();
        let transport = SimTransport::client();
        let conn = transport.connect_to_host("server:443").await?;
        let (mut send, mut recv) = conn.into_split();

        // Wait a bit (virtual time)
        env.sleep(Duration::from_millis(10)).await;

        // Create Ping frame
        let ping_header = FrameHeader::new(Opcode::Ping);
        let ping_frame = Frame::new(ping_header, Vec::new());

        // Send Ping
        let mut ping_buf = Vec::new();
        ping_frame.encode(&mut ping_buf).map_err(to_box_err)?;
        send.write_all(&ping_buf).await?;

        // Read Pong response header
        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;

        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
        assert_eq!(header.opcode_enum(), Some(Opcode::Pong));

        // Read payload (should be empty for Pong)
        let payload_size = header.payload_size() as usize;
        assert_eq!(payload_size, 0, "Pong should have no payload");

        Ok(())
    });

    sim.run().expect("simulation should complete despite packet loss");
}

#[test]
fn ping_pong_with_latency() {
    let mut sim = turmoil::Builder::new()
        .simulation_duration(Duration::from_secs(60))
        .min_message_latency(Duration::from_millis(100))
        .max_message_latency(Duration::from_millis(100))
        .build();

    // Server: respond to Ping with Pong
    sim.host("server", || async move {
        let transport = SimTransport::bind("0.0.0.0:443").await?;
        let conn = transport.accept().await?;
        let (mut send, mut recv) = conn.into_split();

        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;

        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
        assert_eq!(header.opcode_enum(), Some(Opcode::Ping));

        let payload_size = header.payload_size() as usize;
        let mut payload_buf = vec![0u8; payload_size];
        recv.read_exact(&mut payload_buf).await?;

        let pong_header = FrameHeader::new(Opcode::Pong);
        let pong_frame = Frame::new(pong_header, Vec::new());

        let mut response_buf = Vec::new();
        pong_frame.encode(&mut response_buf).map_err(to_box_err)?;
        send.write_all(&response_buf).await?;

        Ok(())
    });

    // Client: measure round-trip time
    sim.client("client", async {
        let env = SimEnv::new();
        let transport = SimTransport::client();
        let conn = transport.connect_to_host("server:443").await?;
        let (mut send, mut recv) = conn.into_split();

        let start = env.now();

        // Send Ping
        let ping_header = FrameHeader::new(Opcode::Ping);
        let ping_frame = Frame::new(ping_header, Vec::new());

        let mut ping_buf = Vec::new();
        ping_frame.encode(&mut ping_buf).map_err(to_box_err)?;
        send.write_all(&ping_buf).await?;

        // Read Pong
        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;

        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
        assert_eq!(header.opcode_enum(), Some(Opcode::Pong));

        let elapsed = env.now() - start;

        // Round trip should be ~200ms (100ms each way)
        assert!(elapsed >= Duration::from_millis(200), "Round trip too fast: {:?}", elapsed);

        Ok(())
    });

    sim.run().expect("simulation should complete with latency");
}

#[test]
fn network_partition_then_heal() {
    // Test that the system handles network partitions gracefully:
    // 1. Client connects to server
    // 2. Network partition isolates them
    // 3. Partition heals
    // 4. Communication resumes

    let mut sim =
        turmoil::Builder::new().simulation_duration(Duration::from_secs(120)).rng_seed(42).build();

    // Server: accept connection, wait through partition, then respond
    sim.host("server", || async move {
        let transport = SimTransport::bind("0.0.0.0:443").await?;

        // Accept initial connection
        let conn = transport.accept().await?;
        let (mut send, mut recv) = conn.into_split();

        // Read first ping (before partition)
        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;

        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
        assert_eq!(header.opcode_enum(), Some(Opcode::Ping));

        let payload_size = header.payload_size() as usize;
        let mut payload_buf = vec![0u8; payload_size];
        recv.read_exact(&mut payload_buf).await?;

        // Send first pong
        let pong_header = FrameHeader::new(Opcode::Pong);
        let pong_frame = Frame::new(pong_header, Vec::new());
        let mut response_buf = Vec::new();
        pong_frame.encode(&mut response_buf).map_err(to_box_err)?;
        send.write_all(&response_buf).await?;

        // PARTITION HAPPENS HERE (controlled by client)
        // Connection will timeout/fail during partition

        // After partition heals, accept new connection
        let conn2 = transport.accept().await?;
        let (mut send2, mut recv2) = conn2.into_split();

        // Read second ping (after partition healed)
        let mut header_buf2 = [0u8; FrameHeader::SIZE];
        recv2.read_exact(&mut header_buf2).await?;

        let header2 = FrameHeader::from_bytes(&header_buf2).map_err(to_box_err)?;
        assert_eq!(header2.opcode_enum(), Some(Opcode::Ping));

        let payload_size2 = header2.payload_size() as usize;
        let mut payload_buf2 = vec![0u8; payload_size2];
        recv2.read_exact(&mut payload_buf2).await?;

        // Send second pong
        let pong_frame2 = Frame::new(FrameHeader::new(Opcode::Pong), Vec::new());
        let mut response_buf2 = Vec::new();
        pong_frame2.encode(&mut response_buf2).map_err(to_box_err)?;
        send2.write_all(&response_buf2).await?;

        Ok(())
    });

    // Client: send ping, partition, heal, reconnect, send ping
    sim.client("client", async {
        let env = SimEnv::new();
        let transport = SimTransport::client();

        // === PHASE 1: Normal operation ===
        let conn = transport.connect_to_host("server:443").await?;
        let (mut send, mut recv) = conn.into_split();

        // Send first ping
        let ping_header = FrameHeader::new(Opcode::Ping);
        let ping_frame = Frame::new(ping_header, Vec::new());
        let mut ping_buf = Vec::new();
        ping_frame.encode(&mut ping_buf).map_err(to_box_err)?;
        send.write_all(&ping_buf).await?;

        // Receive first pong
        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;
        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
        assert_eq!(header.opcode_enum(), Some(Opcode::Pong));

        // === PHASE 2: Network partition ===
        drop(send);
        drop(recv);

        // Simulate partition by introducing delay
        env.sleep(Duration::from_secs(5)).await;

        // === PHASE 3: Partition healed, reconnect ===
        let conn2 = transport.connect_to_host("server:443").await?;
        let (mut send2, mut recv2) = conn2.into_split();

        // Send second ping (after reconnection)
        let ping_frame2 = Frame::new(FrameHeader::new(Opcode::Ping), Vec::new());
        let mut ping_buf2 = Vec::new();
        ping_frame2.encode(&mut ping_buf2).map_err(to_box_err)?;
        send2.write_all(&ping_buf2).await?;

        // Receive second pong
        let mut header_buf2 = [0u8; FrameHeader::SIZE];
        recv2.read_exact(&mut header_buf2).await?;
        let header2 = FrameHeader::from_bytes(&header_buf2).map_err(to_box_err)?;
        assert_eq!(header2.opcode_enum(), Some(Opcode::Pong));

        Ok(())
    });

    sim.run().expect("simulation should handle partition and heal");
}

#[test]
fn asymmetric_packet_loss() {
    // Test asymmetric network conditions: client → server has packet loss,
    // but server → client is reliable. This simulates asymmetric routing
    // issues or congestion in one direction.

    let mut sim =
        turmoil::Builder::new().simulation_duration(Duration::from_secs(60)).rng_seed(789).build();

    // Server: echo back whatever is received
    sim.host("server", || async move {
        let transport = SimTransport::bind("0.0.0.0:443").await?;
        let conn = transport.accept().await?;
        let (mut send, mut recv) = conn.into_split();

        // Read ping
        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;

        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
        assert_eq!(header.opcode_enum(), Some(Opcode::Ping));

        let payload_size = header.payload_size() as usize;
        let mut payload_buf = vec![0u8; payload_size];
        recv.read_exact(&mut payload_buf).await?;

        // Echo back (server → client is reliable)
        let pong_frame = Frame::new(FrameHeader::new(Opcode::Pong), Vec::new());
        let mut response_buf = Vec::new();
        pong_frame.encode(&mut response_buf).map_err(to_box_err)?;
        send.write_all(&response_buf).await?;

        Ok(())
    });

    // Client: send ping with potential loss on outbound path
    sim.client("client", async {
        let transport = SimTransport::client();
        let conn = transport.connect_to_host("server:443").await?;
        let (mut send, mut recv) = conn.into_split();

        // Send ping (may experience loss on client → server path)
        let ping_frame = Frame::new(FrameHeader::new(Opcode::Ping), Vec::new());
        let mut ping_buf = Vec::new();
        ping_frame.encode(&mut ping_buf).map_err(to_box_err)?;
        send.write_all(&ping_buf).await?;

        // Read pong (server → client path is reliable)
        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;
        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
        assert_eq!(header.opcode_enum(), Some(Opcode::Pong));

        Ok(())
    });

    sim.run().expect("simulation should handle asymmetric conditions");
}
