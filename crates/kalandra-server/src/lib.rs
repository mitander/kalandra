//! Kalandra production server.
//!
//! This crate provides the production server implementation using:
//! - Quinn for QUIC transport
//! - Tokio for async runtime
//! - System time and cryptographic RNG
//!
//! ## Architecture
//!
//! ```text
//! kalandra-server
//!   ├─ SystemEnv          (production Environment impl)
//!   ├─ QuinnTransport     (QUIC via Quinn)
//!   ├─ ServerDriver       (Sans-IO orchestrator)
//!   ├─ RoomManager        (MLS validation + sequencing)
//!   ├─ Sequencer          (total ordering)
//!   └─ Storage            (frame persistence)
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod driver;
mod error;
mod executor;
mod registry;
mod room_manager;
mod sequencer;
mod server_error;
pub mod storage;
mod system_env;
mod transport;

use std::sync::Arc;

use bytes::BytesMut;
pub use driver::{LogLevel, ServerAction, ServerConfig as DriverConfig, ServerDriver, ServerEvent};
pub use error::ServerError;
pub use executor::{ActionExecutor, BroadcastPolicy};
use kalandra_proto::{Frame, FrameHeader};
pub use registry::{ConnectionRegistry, SessionInfo};
pub use room_manager::{RoomAction, RoomError, RoomManager, RoomMetadata};
pub use sequencer::{Sequencer, SequencerAction, SequencerError};
pub use server_error::{ExecutorError, ServerError as DriverError};
pub use storage::{ChaoticStorage, MemoryStorage, Storage, StorageError};
pub use system_env::SystemEnv;
pub use transport::{QuinnConnection, QuinnTransport};
use zerocopy::FromBytes;

/// Server configuration for the production runtime.
#[derive(Debug, Clone)]
pub struct ServerRuntimeConfig {
    /// Address to bind to (e.g., "0.0.0.0:4433")
    pub bind_address: String,
    /// Path to TLS certificate (PEM format)
    pub cert_path: Option<String>,
    /// Path to TLS private key (PEM format)
    pub key_path: Option<String>,
    /// Driver configuration (timeouts, limits)
    pub driver: DriverConfig,
}

impl Default for ServerRuntimeConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:4433".to_string(),
            cert_path: None,
            key_path: None,
            driver: DriverConfig::default(),
        }
    }
}

/// Production Kalandra server.
///
/// Wraps `ServerDriver` with Quinn QUIC transport and system environment.
pub struct Server {
    /// The Sans-IO server driver
    driver: ServerDriver<SystemEnv, MemoryStorage>,
    /// QUIC endpoint
    transport: QuinnTransport,
    /// Environment
    env: SystemEnv,
}

impl Server {
    /// Create and bind a new server.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Binding to the address fails
    /// - TLS configuration is invalid
    pub async fn bind(config: ServerRuntimeConfig) -> Result<Self, ServerError> {
        let env = SystemEnv::new();
        let storage = MemoryStorage::new();
        let driver = ServerDriver::new(env.clone(), storage, config.driver);

        let transport =
            QuinnTransport::bind(&config.bind_address, config.cert_path, config.key_path).await?;

        Ok(Self { driver, transport, env })
    }

    /// Run the server, accepting connections and processing frames.
    ///
    /// This method runs until the server is shut down or an error occurs.
    pub async fn run(self) -> Result<(), ServerError> {
        tracing::info!("Server starting on {}", self.transport.local_addr()?);

        let driver = Arc::new(tokio::sync::Mutex::new(self.driver));
        let env = self.env;

        loop {
            match self.transport.accept().await {
                Ok(conn) => {
                    let driver = Arc::clone(&driver);
                    let env = env.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(conn, driver, env).await {
                            tracing::error!("Connection error: {}", e);
                        }
                    });
                },
                Err(e) => {
                    tracing::error!("Accept error: {}", e);
                },
            }
        }
    }

    /// Get the local address the server is bound to.
    pub fn local_addr(&self) -> Result<std::net::SocketAddr, ServerError> {
        self.transport.local_addr()
    }
}

/// Handle a single QUIC connection.
async fn handle_connection(
    conn: QuinnConnection,
    driver: Arc<tokio::sync::Mutex<ServerDriver<SystemEnv, MemoryStorage>>>,
    _env: SystemEnv,
) -> Result<(), ServerError> {
    let conn_id = {
        let mut buf = [0u8; 8];
        getrandom::fill(&mut buf).map_err(|e| ServerError::Internal(e.to_string()))?;
        u64::from_le_bytes(buf)
    };

    tracing::debug!("New connection: {}", conn_id);

    {
        let mut driver = driver.lock().await;
        let actions = driver.process_event(ServerEvent::ConnectionAccepted { conn_id })?;
        execute_actions(&mut *driver, actions, &conn).await?;
    }

    loop {
        match conn.accept_bi().await {
            Ok((send, recv)) => {
                let driver = Arc::clone(&driver);
                let conn = conn.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_stream(conn_id, send, recv, driver, &conn).await {
                        tracing::debug!("Stream error: {}", e);
                    }
                });
            },
            Err(e) => {
                tracing::debug!("Connection closed: {}", e);
                break;
            },
        }
    }

    {
        let mut driver = driver.lock().await;
        let _ = driver.process_event(ServerEvent::ConnectionClosed {
            conn_id,
            reason: "connection closed".to_string(),
        });
    }

    Ok(())
}

/// Handle a single bidirectional stream.
async fn handle_stream(
    conn_id: u64,
    send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    driver: Arc<tokio::sync::Mutex<ServerDriver<SystemEnv, MemoryStorage>>>,
    conn: &QuinnConnection,
) -> Result<(), ServerError> {
    drop(send); // not used for now

    let mut buf = BytesMut::with_capacity(65536);

    loop {
        buf.clear();
        buf.resize(128, 0);

        match recv.read_exact(&mut buf[..128]).await {
            Ok(()) => {},
            Err(e) => {
                tracing::debug!("Read error: {}", e);
                break;
            },
        }

        let header: &FrameHeader = match FrameHeader::ref_from_bytes(&buf[..128]) {
            Ok(h) => h,
            Err(_) => {
                tracing::warn!("Invalid frame header");
                break;
            },
        };

        let payload_size = header.payload_size() as usize;

        if payload_size > 0 {
            buf.resize(128 + payload_size, 0);
            if let Err(e) = recv.read_exact(&mut buf[128..]).await {
                tracing::debug!("Payload read error: {}", e);
                break;
            }
        }

        let frame = match Frame::decode(&buf) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("Frame decode error: {}", e);
                break;
            },
        };

        let actions = {
            let mut driver = driver.lock().await;
            match driver.process_event(ServerEvent::FrameReceived { conn_id, frame }) {
                Ok(actions) => actions,
                Err(e) => {
                    tracing::warn!("Frame processing error: {}", e);
                    continue;
                },
            }
        };

        {
            let mut driver = driver.lock().await;
            execute_actions(&mut *driver, actions, conn).await?;
        }
    }

    Ok(())
}

/// Execute server actions.
async fn execute_actions(
    driver: &mut ServerDriver<SystemEnv, MemoryStorage>,
    actions: Vec<ServerAction<std::time::Instant>>,
    conn: &QuinnConnection,
) -> Result<(), ServerError> {
    for action in actions {
        match action {
            ServerAction::SendToSession { frame, .. } => {
                let mut buf = Vec::new();
                frame.encode(&mut buf).map_err(|e| ServerError::Protocol(e.to_string()))?;

                if let Ok(mut send) = conn.open_uni().await {
                    let _ = send.write_all(&buf).await;
                    let _ = send.finish();
                }
            },

            ServerAction::BroadcastToRoom { room_id, frame, exclude_session } => {
                let sessions: Vec<u64> = driver.sessions_in_room(room_id).collect();

                let mut buf = Vec::new();
                frame.encode(&mut buf).map_err(|e| ServerError::Protocol(e.to_string()))?;

                for session_id in sessions {
                    if Some(session_id) != exclude_session {
                        tracing::debug!("Would broadcast to session {}", session_id);
                    }
                }
            },

            ServerAction::CloseConnection { session_id, reason } => {
                tracing::info!("Closing connection {}: {}", session_id, reason);
            },

            ServerAction::PersistFrame { room_id, log_index, frame } => {
                if let Err(e) = driver.storage().store_frame(room_id, log_index, &frame) {
                    tracing::error!("Failed to persist frame: {}", e);
                }
            },

            ServerAction::PersistMlsState { room_id, state } => {
                if let Err(e) = driver.storage().store_mls_state(room_id, &state) {
                    tracing::error!("Failed to persist MLS state: {}", e);
                }
            },

            ServerAction::Log { level, message, .. } => match level {
                LogLevel::Debug => tracing::debug!("{}", message),
                LogLevel::Info => tracing::info!("{}", message),
                LogLevel::Warn => tracing::warn!("{}", message),
                LogLevel::Error => tracing::error!("{}", message),
            },
        }
    }

    Ok(())
}
