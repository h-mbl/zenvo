//! MCP  server for Zenvo
//! Provides JSON-RPC based API for AI assistants to interact with Zenvo.

pub mod handlers;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};

/// MCP Server configuration
pub struct McpServer {
    pub name: String,
    pub version: String,
}

impl Default for McpServer {
    fn default() -> Self {
        Self {
            name: "zenvo-mcp".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// JSON-RPC request structure
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC response structure
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC error structure
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<serde_json::Value>, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
        }
    }
}

/// MCP Server info response
#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// MCP Tool definition
#[derive(Debug, Serialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

impl McpServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run the MCP server (stdio mode)
    pub fn run(&self) -> Result<()> {
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        let reader = BufReader::new(stdin.lock());

        for line in reader.lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }

            let response = self.handle_request(&line);
            let response_json = serde_json::to_string(&response)?;
            writeln!(stdout, "{}", response_json)?;
            stdout.flush()?;
        }

        Ok(())
    }

    /// Handle a single JSON-RPC request
    fn handle_request(&self, input: &str) -> JsonRpcResponse {
        // Parse the request
        let request: JsonRpcRequest = match serde_json::from_str(input) {
            Ok(r) => r,
            Err(e) => {
                return JsonRpcResponse::error(None, -32700, &format!("Parse error: {}", e));
            }
        };

        // Route to handler
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request.id),
            "tools/list" => self.handle_list_tools(request.id),
            "tools/call" => self.handle_call_tool(request.id, request.params),
            _ => JsonRpcResponse::error(
                request.id,
                -32601,
                &format!("Method not found: {}", request.method),
            ),
        }
    }

    /// Handle initialize request
    fn handle_initialize(&self, id: Option<serde_json::Value>) -> JsonRpcResponse {
        JsonRpcResponse::success(
            id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": self.name,
                    "version": self.version
                },
                "capabilities": {
                    "tools": {}
                }
            }),
        )
    }

    /// Handle list tools request
    fn handle_list_tools(&self, id: Option<serde_json::Value>) -> JsonRpcResponse {
        // Common path property for all tools
        let path_prop = serde_json::json!({
            "type": "string",
            "description": "Path to the Node.js project directory (default: current directory). Use this when the project is in a subdirectory like 'frontend/' or 'client/'."
        });

        let tools = vec![
            Tool {
                name: "detect_node_projects".to_string(),
                description: "Detect Node.js projects in the current directory and subdirectories. Use this first to find where package.json is located.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "get_environment_status".to_string(),
                description: "Get the current Node.js environment status including Node version, package manager, lockfile info, and any issues detected".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": path_prop
                    },
                    "required": []
                }),
            },
            Tool {
                name: "sync_environment".to_string(),
                description: "Update the env.lock file to match the current environment state".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": path_prop,
                        "include_system_info": {
                            "type": "boolean",
                            "description": "Include OS and architecture information"
                        }
                    },
                    "required": []
                }),
            },
            Tool {
                name: "fix_drift".to_string(),
                description: "Generate and optionally execute a repair plan to fix environment drift".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": path_prop,
                        "execute": {
                            "type": "boolean",
                            "description": "Whether to execute the repair plan (default: false, only shows plan)"
                        },
                        "safe_only": {
                            "type": "boolean",
                            "description": "Only execute safe repairs that don't require confirmation (default: true)"
                        }
                    },
                    "required": []
                }),
            },
            Tool {
                name: "run_doctor".to_string(),
                description: "Run diagnostic checks on the Node.js environment and return detailed results".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": path_prop,
                        "category": {
                            "type": "string",
                            "enum": ["toolchain", "lockfile", "deps", "frameworks"],
                            "description": "Only run checks in this category"
                        }
                    },
                    "required": []
                }),
            },
            Tool {
                name: "search_versions".to_string(),
                description: "Search for available versions of an npm package. Use this to find correct version numbers when a package version is not found or to check compatibility.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "package": {
                            "type": "string",
                            "description": "Package name to search (e.g., 'express', '@types/node', 'expo-notifications')"
                        },
                        "constraint": {
                            "type": "string",
                            "description": "Filter versions by constraint (e.g., '^18.0.0', '~0.31', '>=1.0.0')"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Number of versions to return (default: 10)"
                        }
                    },
                    "required": ["package"]
                }),
            },
            Tool {
                name: "resolve_conflicts".to_string(),
                description: "Detect and resolve npm peer dependency conflicts. Analyzes npm install errors, searches for compatible package versions, and can automatically update package.json.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": path_prop,
                        "apply": {
                            "type": "boolean",
                            "description": "If true, automatically update package.json with resolved versions (default: false, only shows suggestions)"
                        }
                    },
                    "required": []
                }),
            },
        ];

        JsonRpcResponse::success(id, serde_json::json!({ "tools": tools }))
    }

    /// Handle tool call request
    fn handle_call_tool(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let name = params
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("");

        let arguments = params.get("arguments").cloned().unwrap_or(serde_json::json!({}));

        let result = match name {
            "detect_node_projects" => handlers::detect_node_projects(&arguments),
            "get_environment_status" => handlers::get_environment_status(&arguments),
            "sync_environment" => handlers::sync_environment(&arguments),
            "fix_drift" => handlers::fix_drift(&arguments),
            "run_doctor" => handlers::run_doctor(&arguments),
            "search_versions" => handlers::search_versions(&arguments),
            "resolve_conflicts" => handlers::resolve_conflicts(&arguments),
            _ => {
                return JsonRpcResponse::error(
                    id,
                    -32602,
                    &format!("Unknown tool: {}", name),
                );
            }
        };

        match result {
            Ok(content) => JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&content).unwrap_or_default()
                    }]
                }),
            ),
            Err(e) => JsonRpcResponse::error(id, -32000, &e.to_string()),
        }
    }
}
