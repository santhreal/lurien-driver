use std::net::SocketAddr;

use anyhow::Context as _;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bind: SocketAddr = std::env::args()
        .nth(1)
        .as_deref()
        .unwrap_or("127.0.0.1:8443")
        .parse()
        .context("parse bind address")?;

    guise_echo::serve(bind).await
}
