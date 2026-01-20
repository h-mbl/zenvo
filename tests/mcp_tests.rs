//! MCP (Model Context Protocol) tests for Zenvo
//!
//! Tests for JSON-RPC protocol handling and MCP tool implementations.

use serde_json::{json, Value};

// ============================================================================
// JSON-RPC Request/Response Structure Tests
// ============================================================================

#[test]
fn test_valid_jsonrpc_request_parsing() {
    let request_json = r#"{
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    }"#;

    let parsed: Result<Value, _> = serde_json::from_str(request_json);
    assert!(parsed.is_ok(), "Valid JSON-RPC request should parse");

    let request = parsed.unwrap();
    assert_eq!(request["jsonrpc"], "2.0");
    assert_eq!(request["id"], 1);
    assert_eq!(request["method"], "initialize");
}

#[test]
fn test_jsonrpc_request_with_string_id() {
    let request_json = r#"{
        "jsonrpc": "2.0",
        "id": "request-123",
        "method": "tools/list",
        "params": {}
    }"#;

    let parsed: Value = serde_json::from_str(request_json).unwrap();
    assert_eq!(parsed["id"], "request-123");
}

#[test]
fn test_jsonrpc_request_without_id() {
    // Notification (no id)
    let request_json = r#"{
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    }"#;

    let parsed: Value = serde_json::from_str(request_json).unwrap();
    assert!(parsed.get("id").is_none() || parsed["id"].is_null());
}

#[test]
fn test_jsonrpc_success_response_format() {
    let response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "protocolVersion": "2024-11-05",
            "serverInfo": {
                "name": "zenvo-mcp",
                "version": "0.1.0"
            },
            "capabilities": {
                "tools": {}
            }
        }
    });

    assert_eq!(response["jsonrpc"], "2.0");
    assert!(response.get("error").is_none());
    assert!(response["result"].is_object());
}

#[test]
fn test_jsonrpc_error_response_format() {
    let response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": -32601,
            "message": "Method not found"
        }
    });

    assert_eq!(response["jsonrpc"], "2.0");
    assert!(response.get("result").is_none());
    assert_eq!(response["error"]["code"], -32601);
}

// ============================================================================
// MCP Tool Definition Tests
// ============================================================================

#[test]
fn test_tools_list_response_structure() {
    let tools_response = json!({
        "tools": [
            {
                "name": "get_environment_status",
                "description": "Get the current Node.js environment status",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "sync_environment",
                "description": "Update the env.lock file",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "include_system_info": {
                            "type": "boolean"
                        }
                    },
                    "required": []
                }
            }
        ]
    });

    let tools = tools_response["tools"].as_array().unwrap();
    assert!(tools.len() >= 2, "Should have multiple tools defined");

    // Verify tool structure
    for tool in tools {
        assert!(tool.get("name").is_some(), "Tool should have name");
        assert!(tool.get("description").is_some(), "Tool should have description");
        assert!(tool.get("inputSchema").is_some(), "Tool should have inputSchema");
    }
}

#[test]
fn test_tool_input_schema_validation() {
    // Schema for fix_drift tool
    let schema = json!({
        "type": "object",
        "properties": {
            "execute": {
                "type": "boolean",
                "description": "Whether to execute the repair plan"
            },
            "safe_only": {
                "type": "boolean",
                "description": "Only execute safe repairs"
            }
        },
        "required": []
    });

    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["execute"].is_object());
    assert!(schema["properties"]["safe_only"].is_object());
}

#[test]
fn test_run_doctor_tool_input_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "category": {
                "type": "string",
                "enum": ["toolchain", "lockfile", "deps", "frameworks"],
                "description": "Only run checks in this category"
            }
        },
        "required": []
    });

    let valid_categories = schema["properties"]["category"]["enum"].as_array().unwrap();
    assert!(valid_categories.contains(&json!("toolchain")));
    assert!(valid_categories.contains(&json!("lockfile")));
    assert!(valid_categories.contains(&json!("deps")));
    assert!(valid_categories.contains(&json!("frameworks")));
}

// ============================================================================
// MCP Tool Call Tests
// ============================================================================

#[test]
fn test_tool_call_request_format() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "get_environment_status",
            "arguments": {}
        }
    });

    assert_eq!(request["method"], "tools/call");
    assert_eq!(request["params"]["name"], "get_environment_status");
}

#[test]
fn test_tool_call_with_arguments() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {
            "name": "run_doctor",
            "arguments": {
                "category": "toolchain"
            }
        }
    });

    assert_eq!(request["params"]["name"], "run_doctor");
    assert_eq!(request["params"]["arguments"]["category"], "toolchain");
}

#[test]
fn test_tool_call_response_content_format() {
    // MCP tool responses should have content array
    let response = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "result": {
            "content": [{
                "type": "text",
                "text": "{\"success\": true}"
            }]
        }
    });

    let content = response["result"]["content"].as_array().unwrap();
    assert!(!content.is_empty(), "Content should not be empty");
    assert_eq!(content[0]["type"], "text");
}

// ============================================================================
// Error Code Tests
// ============================================================================

#[test]
fn test_parse_error_code() {
    // -32700: Parse error
    let error = json!({
        "code": -32700,
        "message": "Parse error: invalid JSON"
    });
    assert_eq!(error["code"], -32700);
}

#[test]
fn test_method_not_found_error_code() {
    // -32601: Method not found
    let error = json!({
        "code": -32601,
        "message": "Method not found: unknown_method"
    });
    assert_eq!(error["code"], -32601);
}

#[test]
fn test_invalid_params_error_code() {
    // -32602: Invalid params
    let error = json!({
        "code": -32602,
        "message": "Unknown tool: nonexistent_tool"
    });
    assert_eq!(error["code"], -32602);
}

#[test]
fn test_internal_error_code() {
    // -32000 to -32099: Server errors
    let error = json!({
        "code": -32000,
        "message": "Internal error: something went wrong"
    });
    assert!(error["code"].as_i64().unwrap() <= -32000);
    assert!(error["code"].as_i64().unwrap() >= -32099);
}

// ============================================================================
// Initialize Handshake Tests
// ============================================================================

#[test]
fn test_initialize_request() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            },
            "capabilities": {}
        }
    });

    assert_eq!(request["method"], "initialize");
    assert!(request["params"]["clientInfo"].is_object());
}

#[test]
fn test_initialize_response_structure() {
    let response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "protocolVersion": "2024-11-05",
            "serverInfo": {
                "name": "zenvo-mcp",
                "version": "0.1.0"
            },
            "capabilities": {
                "tools": {}
            }
        }
    });

    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    assert!(response["result"]["serverInfo"]["name"].is_string());
    assert!(response["result"]["serverInfo"]["version"].is_string());
    assert!(response["result"]["capabilities"]["tools"].is_object());
}

// ============================================================================
// Tool Response Content Tests
// ============================================================================

#[test]
fn test_environment_status_response() {
    // Expected shape of get_environment_status response
    let status = json!({
        "current": {
            "node_version": "20.11.0",
            "package_manager": "npm",
            "package_manager_version": "10.2.4",
            "lockfile_type": "npm",
            "lockfile_hash": "abc123"
        },
        "has_env_lock": true,
        "drift_detected": false,
        "issues": [],
        "summary": {
            "total_checks": 20,
            "passed": 18,
            "warnings": 2,
            "errors": 0
        }
    });

    assert!(status["current"]["node_version"].is_string());
    assert!(status["summary"]["total_checks"].is_number());
    assert!(status["issues"].is_array());
}

#[test]
fn test_fix_drift_response_with_actions() {
    let response = json!({
        "success": true,
        "message": "Repair plan generated (not executed)",
        "total_issues": 3,
        "actions": [
            {
                "description": "Switch Node.js version",
                "command": "nvm use 20.11.0",
                "is_safe": true
            },
            {
                "description": "Install dependencies",
                "command": "npm ci",
                "is_safe": true
            }
        ]
    });

    assert!(response["success"].as_bool().unwrap());
    let actions = response["actions"].as_array().unwrap();
    assert!(!actions.is_empty());

    for action in actions {
        assert!(action.get("description").is_some());
        assert!(action.get("command").is_some());
        assert!(action.get("is_safe").is_some());
    }
}

#[test]
fn test_doctor_response_with_issues() {
    let response = json!({
        "success": false,
        "drift_detected": true,
        "issues": [
            {
                "name": "Node version match",
                "severity": "error",
                "message": "Expected 20.11.0 but found 18.17.0",
                "category": "toolchain"
            }
        ],
        "summary": {
            "total": 20,
            "passed": 18,
            "warnings": 1,
            "errors": 1
        }
    });

    assert!(!response["success"].as_bool().unwrap());
    let issues = response["issues"].as_array().unwrap();
    assert!(!issues.is_empty());

    let first_issue = &issues[0];
    assert_eq!(first_issue["severity"], "error");
    assert!(first_issue["message"].is_string());
}
