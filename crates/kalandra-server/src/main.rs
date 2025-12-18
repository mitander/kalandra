//! Kalandra server binary.
//!
//! # Usage
//!
//! ```bash
//! # Start with self-signed certificate (development)
//! kalandra-server --bind 0.0.0.0:4433
//!
//! # Start with TLS certificate (production)
//! kalandra-server --bind 0.0.0.0:4433 --cert cert.pem --key key.pem
//! ```

use clap::Parser;
use kalandra_server::{DriverConfig, Server, ServerRuntimeConfig};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Kalandra protocol server
#[derive(Parser, Debug)]
#[command(name = "kalandra-server")]
#[command(about = "Kalandra messaging protocol server")]
#[command(version)]
struct Args {
    /// Address to bind to
    #[arg(short, long, default_value = "0.0.0.0:4433")]
    bind: String,

    /// Path to TLS certificate (PEM format)
    #[arg(short, long)]
    cert: Option<String>,

    /// Path to TLS private key (PEM format)
    #[arg(short, long)]
    key: Option<String>,

    /// Maximum concurrent connections
    #[arg(long, default_value = "10000")]
    max_connections: usize,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&args.log_level));

    tracing_subscriber::registry().with(fmt::layer()).with(filter).init();

    tracing::info!("Kalandra server starting");
    tracing::info!("Binding to {}", args.bind);

    if args.cert.is_none() || args.key.is_none() {
        tracing::warn!("No TLS certificate provided - using self-signed certificate");
        tracing::warn!("This is NOT suitable for production use!");
    }

    let config = ServerRuntimeConfig {
        bind_address: args.bind,
        cert_path: args.cert,
        key_path: args.key,
        driver: DriverConfig { max_connections: args.max_connections, ..Default::default() },
    };

    let server = Server::bind(config).await?;

    tracing::info!("Server listening on {}", server.local_addr()?);

    server.run().await?;

    Ok(())
}
