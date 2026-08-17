//! lurien-mcp. Stdio JSON-RPC. Playwright-MCP names. No challenge tool.

use lurien::mcp::McpServer;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Product never skips. Missing engine is exit 1 on stdio.
    if let Err(e) = lurien::resolve_engine_checked() {
        eprintln!("{e}");
        std::process::exit(1);
    }
    let server = McpServer::new();
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();
    while let Some(line) = lines.next_line().await? {
        if let Some(resp) = server.handle_line(&line).await {
            stdout.write_all(resp.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}
