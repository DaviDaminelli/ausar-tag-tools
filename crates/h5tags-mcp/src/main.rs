//! `h5tags-mcp`: the h5tags tools over MCP, on stdio (PRD §9a). Launched by
//! an MCP client (Claude Code, Claude Desktop). stdout carries the protocol;
//! logs go to stderr only.

mod server;

use std::process::ExitCode;
use std::sync::{Arc, Mutex, OnceLock};

use clap::Parser;
use h5tags_core::Session;
use h5tags_core::config::ConfigArgs;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use server::{Server, SessionSlot};

/// The h5tags tools for AI clients, over MCP on stdio.
#[derive(Parser)]
#[command(name = "h5tags-mcp", version)]
struct Args {
    #[command(flatten)]
    config: ConfigArgs,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(EnvFilter::try_from_env("H5TAGS_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let config = match args.config.load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Indexing can take a while on a first run: answer the client at once,
    // and let tool calls wait for the session.
    let slot: SessionSlot = Arc::new(OnceLock::new());
    let opening = slot.clone();
    std::thread::spawn(move || {
        let session = Session::open(config).map(Mutex::new).map_err(|e| e.to_string());
        match &session {
            Ok(_) => tracing::info!("session ready"),
            Err(e) => tracing::error!(error = %e, "can't open the session"),
        }
        let _ = opening.set(session);
    });
    let service = match Server::new(slot).serve(rmcp::transport::stdio()).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "can't start the MCP server");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = service.waiting().await {
        tracing::error!(error = %e, "the MCP server stopped");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
