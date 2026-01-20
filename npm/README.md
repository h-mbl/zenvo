# Zenvo

> Node.js environment lock, doctor & repair tool

Like Poetry for Python, but for the entire Node.js toolchain. Generates `env.lock`, detects drift, and provides guided repair.

## Installation

```bash
npm install -g zenvo
```

## Quick Start

```bash
# Initialize in your project
zenvo init

# Run diagnostics
zenvo doctor

# Fix issues
zenvo repair --plan
zenvo repair --apply

# Resolve peer dependency conflicts
zenvo resolve
```

## Features

- **Lock**: Generate `env.lock` file with environment fingerprint
- **Doctor**: 20+ diagnostic checks for Node.js environment
- **Repair**: Guided fixes with reviewable action plans
- **Resolve**: Automatic peer dependency conflict resolution
- **Verify**: CI/CD mode for environment verification
- **MCP Server**: AI assistant integration (Claude, Cursor, etc.)

## Commands

```bash
zenvo init              # Initialize project
zenvo lock              # Generate env.lock
zenvo doctor            # Run diagnostics
zenvo repair --plan     # Show repair plan
zenvo repair --apply    # Execute repairs
zenvo resolve           # Fix peer dep conflicts
zenvo verify            # CI mode check
zenvo versions <pkg>    # Search npm versions
zenvo clean             # Clean caches
```

## MCP Server (AI Integration)

Zenvo includes an MCP server for AI assistants like Claude Desktop or Cursor.

After installing via npm, configure Claude Desktop (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "zenvo": {
      "command": "zenvo-mcp"
    }
  }
}
```

The MCP server exposes 7 tools:
- `detect_node_projects` - Find Node.js projects
- `get_environment_status` - Check environment health
- `sync_environment` - Update env.lock
- `fix_drift` - Repair environment issues
- `run_doctor` - Run diagnostic checks
- `search_versions` - Search npm package versions
- `resolve_conflicts` - Fix peer dependency conflicts

## Alternative Installation

```bash
# Homebrew (macOS/Linux)
brew install h-mbl/zenvo/zenvo

# Cargo (Rust)
cargo install zenvo
```

## Supported Platforms

- macOS (Intel & Apple Silicon)
- Linux (x64 & ARM64)
- Windows (x64)

## Documentation

See [full documentation](https://github.com/h-mbl/zenvo#readme) for more details.

## Issues

[Report issues on GitHub](https://github.com/h-mbl/zenvo/issues)

## License

MIT
