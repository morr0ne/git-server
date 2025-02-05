use std::net::Ipv4Addr;

use anyhow::Result;
use axum::{routing::get, Router};
use tokio::net::TcpListener;
use tracing::debug;

const PORT: u16 = 3344;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let app = Router::new().route("/", get(root));

    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT)).await?;

    debug!("Started server on port {PORT}");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn root() -> &'static str {
    "Hello, World!"
}
