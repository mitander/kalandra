//! Ping-pong protocol test using Lockframe frames.
//!
//! This test validates:
//! - Frame serialization/deserialization over the network
//! - Bidirectional communication with proper frame handling
//! - Virtual time advancement in simulation
//! - Basic protocol flow (Ping request -> Pong response)

use std::time::Duration;

use lockframe_core::{env::Environment, transport::Transport};
use lockframe_harness::{SimEnv, SimTransport};
use lockframe_proto::{
    Frame, FrameHeader, Opcode, Payload,
    payloads::session::{Hello, HelloReply},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Helper to convert any error to Box<dyn Error>
fn to_box_err<E: std::error::Error + 'static>(e: E) -> Box<dyn std::error::Error> {
    Box::new(e)
}

#[test]
fn ping_pong_basic() {
    let mut sim = turmoil::Builder::new().build();

    // Server: respond to Ping with Pong
    sim.host("server", || async move {
        let transport = SimTransport::bind("0.0.0.0:443").await?;
        let conn = transport.accept().await?;
        let (mut send, mut recv) = conn.into_split();

        // Read frame header (128 bytes)
        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;

        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;

        // Verify it's a Ping
        assert_eq!(header.opcode_enum(), Some(Opcode::Ping));

        // Read payload
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

        // Verify it's a Pong
        assert_eq!(header.opcode_enum(), Some(Opcode::Pong));

        // Read payload (should be empty for Pong)
        let payload_size = header.payload_size() as usize;
        assert_eq!(payload_size, 0, "Pong should have no payload");

        Ok(())
    });

    sim.run().expect("simulation should complete successfully");
}

#[test]
fn ping_pong_with_payload() {
    let mut sim = turmoil::Builder::new().build();

    // Server: echo back Hello message
    sim.host("server", || async move {
        let transport = SimTransport::bind("0.0.0.0:443").await?;
        let conn = transport.accept().await?;
        let (mut send, mut recv) = conn.into_split();

        // Read frame
        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;
        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;

        let payload_size = header.payload_size() as usize;
        let mut payload_buf = vec![0u8; payload_size];
        recv.read_exact(&mut payload_buf).await?;

        let frame = Frame::new(*header, payload_buf);

        // Parse payload
        let payload = Payload::from_frame(frame).map_err(to_box_err)?;

        // Verify it's a Hello
        match payload {
            Payload::Hello(hello) => {
                assert_eq!(hello.version, 1);

                // Send HelloReply
                let reply = Payload::HelloReply(HelloReply {
                    session_id: 0x1234_5678_9ABC_DEF0,
                    capabilities: vec![],
                    challenge: None,
                });

                let reply_frame =
                    reply.into_frame(FrameHeader::new(Opcode::HelloReply)).map_err(to_box_err)?;

                let mut reply_buf = Vec::new();
                reply_frame.encode(&mut reply_buf).map_err(to_box_err)?;
                send.write_all(&reply_buf).await?;

                Ok(())
            },
            _ => Err(to_box_err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "expected Hello",
            ))),
        }
    });

    // Client: send Hello, expect HelloReply
    sim.client("client", async {
        let transport = SimTransport::client();
        let conn = transport.connect_to_host("server:443").await?;
        let (mut send, mut recv) = conn.into_split();

        // Create Hello payload
        let hello = Payload::Hello(Hello { version: 1, capabilities: vec![], auth_token: None });

        let hello_frame = hello.into_frame(FrameHeader::new(Opcode::Hello)).map_err(to_box_err)?;

        // Send Hello
        let mut hello_buf = Vec::new();
        hello_frame.encode(&mut hello_buf).map_err(to_box_err)?;
        send.write_all(&hello_buf).await?;

        // Read HelloReply
        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;
        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;

        assert_eq!(header.opcode_enum(), Some(Opcode::HelloReply));

        let payload_size = header.payload_size() as usize;
        let mut payload_buf = vec![0u8; payload_size];
        recv.read_exact(&mut payload_buf).await?;

        let frame = Frame::new(*header, payload_buf);
        let payload = Payload::from_frame(frame).map_err(to_box_err)?;

        // Verify HelloReply
        match payload {
            Payload::HelloReply(reply) => {
                assert_eq!(reply.session_id, 0x1234_5678_9ABC_DEF0);
                Ok(())
            },
            _ => Err(to_box_err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "expected HelloReply",
            ))),
        }
    });

    sim.run().expect("simulation should complete successfully");
}

#[test]
fn ping_pong_multiple_clients() {
    let mut sim = turmoil::Builder::new().build();

    // Server: handle multiple clients
    sim.host("server", || async move {
        let transport = SimTransport::bind("0.0.0.0:443").await?;

        // Accept first client
        let conn1 = transport.accept().await?;
        let (mut send1, mut recv1) = conn1.into_split();

        // Accept second client
        let conn2 = transport.accept().await?;
        let (mut send2, mut recv2) = conn2.into_split();

        // Handle both clients concurrently
        let task1 = async {
            let mut header_buf = [0u8; FrameHeader::SIZE];
            recv1.read_exact(&mut header_buf).await?;
            let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
            assert_eq!(header.opcode_enum(), Some(Opcode::Ping));

            let pong = Frame::new(FrameHeader::new(Opcode::Pong), Vec::new());
            let mut buf = Vec::new();
            pong.encode(&mut buf).map_err(to_box_err)?;
            send1.write_all(&buf).await?;
            Ok::<_, Box<dyn std::error::Error>>(())
        };

        let task2 = async {
            let mut header_buf = [0u8; FrameHeader::SIZE];
            recv2.read_exact(&mut header_buf).await?;
            let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
            assert_eq!(header.opcode_enum(), Some(Opcode::Ping));

            let pong = Frame::new(FrameHeader::new(Opcode::Pong), Vec::new());
            let mut buf = Vec::new();
            pong.encode(&mut buf).map_err(to_box_err)?;
            send2.write_all(&buf).await?;
            Ok::<_, Box<dyn std::error::Error>>(())
        };

        // Run both tasks concurrently
        tokio::try_join!(task1, task2)?;

        Ok(())
    });

    // Client 1
    sim.client("client1", async {
        let transport = SimTransport::client();
        let conn = transport.connect_to_host("server:443").await?;
        let (mut send, mut recv) = conn.into_split();

        let ping = Frame::new(FrameHeader::new(Opcode::Ping), Vec::new());
        let mut buf = Vec::new();
        ping.encode(&mut buf).map_err(to_box_err)?;
        send.write_all(&buf).await?;

        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;
        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
        assert_eq!(header.opcode_enum(), Some(Opcode::Pong));

        Ok(())
    });

    // Client 2
    sim.client("client2", async {
        let transport = SimTransport::client();
        let conn = transport.connect_to_host("server:443").await?;
        let (mut send, mut recv) = conn.into_split();

        let ping = Frame::new(FrameHeader::new(Opcode::Ping), Vec::new());
        let mut buf = Vec::new();
        ping.encode(&mut buf).map_err(to_box_err)?;
        send.write_all(&buf).await?;

        let mut header_buf = [0u8; FrameHeader::SIZE];
        recv.read_exact(&mut header_buf).await?;
        let header = FrameHeader::from_bytes(&header_buf).map_err(to_box_err)?;
        assert_eq!(header.opcode_enum(), Some(Opcode::Pong));

        Ok(())
    });

    sim.run().expect("simulation should complete successfully");
}
