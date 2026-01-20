//! Zenvo MCP Server Binary
//! This binary provides a JSON-RPC server for AI assistants to interact with Zenvo using the MCP
//!
//! ## Usage
//! The server communicates via stdio, reading JSON-RPC requests from stdin
//! and writing responses to stdout.
//!
//! ```bash
//! zenvo-mcp
//! ```
//!
//! ## Available Tools
//!
//! - `get_environment_status` - Get current environment status and issues
//! - `sync_environment` - Update env.lock to match current state
//! - `fix_drift` - Generate and execute repair plan
//! - `run_doctor` - Run diagnostic checks

use zenvo::mcp::McpServer;

fn main() {
    let server = McpServer::new();

    if let Err(e) = server.run() {
        eprintln!("MCP server error: {}", e);
        std::process::exit(1);
    }
}
