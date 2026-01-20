use anyhow::Result;

use super::{CheckResult, CurrentEnvironment};
use crate::lockfile::EnvLock;
use crate::utils::{run_command_with_timeout, CommandResult, SHORT_COMMAND_TIMEOUT};

/// Detected Node.js version manager
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeVersionManager {
    Volta,
    Fnm,
    Nvm,
    System,
    Unknown,
}

impl std::fmt::Display for NodeVersionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeVersionManager::Volta => write!(f, "volta"),
            NodeVersionManager::Fnm => write!(f, "fnm"),
            NodeVersionManager::Nvm => write!(f, "nvm"),
            NodeVersionManager::System => write!(f, "system"),
            NodeVersionManager::Unknown => write!(f, "unknown"),
        }
    }
}

/// Detect which Node version manager is active
pub fn detect_node_version_manager() -> NodeVersionManager {
    // Check for Volta first (it sets VOLTA_HOME)
    if std::env::var("VOLTA_HOME").is_ok() {
        // Verify volta is actually managing Node
        if let CommandResult::Success(_) =
            run_command_with_timeout("volta", &["which", "node"], SHORT_COMMAND_TIMEOUT)
        {
            return NodeVersionManager::Volta;
        }
    }

    // Check for fnm (it sets FNM_MULTISHELL_PATH or FNM_DIR)
    if std::env::var("FNM_MULTISHELL_PATH").is_ok() || std::env::var("FNM_DIR").is_ok() {
        return NodeVersionManager::Fnm;
    }

    // Check for nvm (it sets NVM_DIR)
    if std::env::var("NVM_DIR").is_ok() {
        return NodeVersionManager::Nvm;
    }

    // Check by looking at the node binary path (Unix)
    #[cfg(not(target_os = "windows"))]
    if let CommandResult::Success(output) =
        run_command_with_timeout("which", &["node"], SHORT_COMMAND_TIMEOUT)
    {
        let path = String::from_utf8_lossy(&output.stdout).to_lowercase();
        if path.contains("volta") {
            return NodeVersionManager::Volta;
        }
        if path.contains("fnm") {
            return NodeVersionManager::Fnm;
        }
        if path.contains("nvm") || path.contains(".nvm") {
            return NodeVersionManager::Nvm;
        }
    }

    // Windows: use where instead of which
    #[cfg(target_os = "windows")]
    if let CommandResult::Success(output) =
        run_command_with_timeout("where", &["node"], SHORT_COMMAND_TIMEOUT)
    {
        let path = String::from_utf8_lossy(&output.stdout).to_lowercase();
        if path.contains("volta") {
            return NodeVersionManager::Volta;
        }
        if path.contains("fnm") {
            return NodeVersionManager::Fnm;
        }
        if path.contains("nvm") {
            return NodeVersionManager::Nvm;
        }
    }

    // Check if node exists at all
    if let CommandResult::Success(_) =
        run_command_with_timeout("node", &["--version"], SHORT_COMMAND_TIMEOUT)
    {
        return NodeVersionManager::System;
    }

    NodeVersionManager::Unknown
}

pub fn detect_node_version() -> Result<String> {
    match run_command_with_timeout("node", &["--version"], SHORT_COMMAND_TIMEOUT) {
        CommandResult::Success(output) => {
            let version = String::from_utf8_lossy(&output.stdout)
                .trim()
                .trim_start_matches('v')
                .to_string();

            if version.is_empty() {
                anyhow::bail!("node --version returned empty output");
            }

            Ok(version)
        }
        CommandResult::Failed(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("node --version failed: {}", stderr.trim())
        }
        CommandResult::TimedOut => {
            anyhow::bail!("node --version timed out - Node.js may be hanging or unresponsive")
        }
        CommandResult::SpawnError(e) => {
            anyhow::bail!(
                "Failed to execute 'node --version'. Is Node.js installed and in PATH? Error: {}",
                e
            )
        }
    }
}

/// Detect Node version with the version manager source
pub fn detect_node_version_with_source() -> Result<(String, NodeVersionManager)> {
    let version = detect_node_version()?;
    let manager = detect_node_version_manager();
    Ok((version, manager))
}

pub fn detect_package_manager() -> Result<(String, String)> {
    // Check for packageManager field in package.json first
    if let Ok(pkg_json) = std::fs::read_to_string("package.json") {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&pkg_json) {
            if let Some(pm) = pkg.get("packageManager").and_then(|v| v.as_str()) {
                // Format: "pnpm@8.15.1"
                if let Some((name, version)) = pm.split_once('@') {
                    return Ok((name.to_string(), version.to_string()));
                }
            }
        }
    }

    // Detect by lockfile presence
    if std::path::Path::new("pnpm-lock.yaml").exists() {
        let version = get_tool_version("pnpm")?;
        return Ok(("pnpm".to_string(), version));
    }

    if std::path::Path::new("yarn.lock").exists() {
        let version = get_tool_version("yarn")?;
        return Ok(("yarn".to_string(), version));
    }

    // Default to npm
    let version = get_tool_version("npm")?;
    Ok(("npm".to_string(), version))
}

fn get_tool_version(tool: &str) -> Result<String> {
    match run_command_with_timeout(tool, &["--version"], SHORT_COMMAND_TIMEOUT) {
        CommandResult::Success(output) => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();

            if version.is_empty() {
                anyhow::bail!("{} --version returned empty output", tool);
            }

            Ok(version)
        }
        CommandResult::Failed(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("{} --version failed: {}", tool, stderr.trim())
        }
        CommandResult::TimedOut => {
            anyhow::bail!(
                "{} --version timed out - {} may be hanging or unresponsive",
                tool,
                tool
            )
        }
        CommandResult::SpawnError(e) => {
            anyhow::bail!(
                "Failed to execute '{} --version'. Is {} installed? Error: {}",
                tool,
                tool,
                e
            )
        }
    }
}

/// Detect if corepack is enabled and properly configured
/// Returns:
/// - Some(true) if corepack is available AND packageManager field is set
/// - Some(false) if corepack is available but packageManager is not set
/// - None if corepack is not available or timed out
pub fn detect_corepack_enabled() -> Option<bool> {
    // Check if corepack is available
    match run_command_with_timeout("corepack", &["--version"], SHORT_COMMAND_TIMEOUT) {
        CommandResult::Success(_) => {
            // Corepack is available, check if packageManager field is set
            if let Ok(pkg_json) = std::fs::read_to_string("package.json") {
                if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&pkg_json) {
                    if pkg.get("packageManager").is_some() {
                        return Some(true);
                    }
                }
            }
            // Corepack available but not enforced via packageManager
            Some(false)
        }
        _ => None, // Corepack not available, failed, or timed out
    }
}

pub fn run_checks(current: &CurrentEnvironment, env_lock: &Option<EnvLock>) -> Result<Vec<CheckResult>> {
    let mut results = Vec::new();

    // Check 1: Node.js is accessible
    if current.node_version.is_empty() {
        results.push(
            CheckResult::error("Node.js accessible", "toolchain", "Node.js not found in PATH")
                .with_fix("Install Node.js or check your PATH")
        );
    } else {
        results.push(CheckResult::pass("Node.js accessible", "toolchain"));
    }

    // Check 2: Package manager is accessible
    let pm_check = check_package_manager_accessible(&current.package_manager);
    results.push(pm_check);

    // Check 3: Node version matches env.lock
    if let Some(lock) = env_lock {
        if current.node_version != lock.toolchain.node {
            results.push(
                CheckResult::error(
                    "Node version match",
                    "toolchain",
                    &format!(
                        "Expected {} but found {}",
                        lock.toolchain.node, current.node_version
                    ),
                )
                .with_fix(&format!("nvm use {} or volta pin node@{}", lock.toolchain.node, lock.toolchain.node))
            );
        } else {
            results.push(CheckResult::pass("Node version match", "toolchain"));
        }

        // Check 3: Package manager matches
        if current.package_manager != lock.toolchain.package_manager {
            results.push(
                CheckResult::error(
                    "Package manager match",
                    "toolchain",
                    &format!(
                        "Expected {} but found {}",
                        lock.toolchain.package_manager, current.package_manager
                    ),
                )
                .with_fix(&format!("Use {} instead", lock.toolchain.package_manager))
            );
        } else {
            results.push(CheckResult::pass("Package manager match", "toolchain"));
        }

        // Check 4: Package manager version
        if current.package_manager_version != lock.toolchain.package_manager_version {
            results.push(
                CheckResult::warning(
                    "Package manager version",
                    "toolchain",
                    &format!(
                        "Expected {} but found {}",
                        lock.toolchain.package_manager_version,
                        current.package_manager_version
                    ),
                )
            );
        } else {
            results.push(CheckResult::pass("Package manager version", "toolchain"));
        }
    }

    // Check 5: Corepack status (available and enabled)
    let corepack_result = check_corepack_status();
    results.push(corepack_result);

    // Check 6: Engines field compliance
    if let Some(engines_result) = check_engines_compliance(current) {
        results.push(engines_result);
    }

    Ok(results)
}

/// Check if the package manager is accessible
fn check_package_manager_accessible(pm: &str) -> CheckResult {
    match run_command_with_timeout(pm, &["--version"], SHORT_COMMAND_TIMEOUT) {
        CommandResult::Success(_) => {
            CheckResult::pass(&format!("{} accessible", pm), "toolchain")
        }
        CommandResult::Failed(_) => CheckResult::error(
            &format!("{} accessible", pm),
            "toolchain",
            &format!("{} command failed", pm),
        )
        .with_fix(&format!("Install {} or check your PATH", pm)),
        CommandResult::TimedOut => CheckResult::error(
            &format!("{} accessible", pm),
            "toolchain",
            &format!("{} command timed out - may be hanging or unresponsive", pm),
        )
        .with_fix(&format!("Check if {} is working correctly", pm)),
        CommandResult::SpawnError(_) => CheckResult::error(
            &format!("{} accessible", pm),
            "toolchain",
            &format!("{} not found in PATH", pm),
        )
        .with_fix(&format!("Install {} or check your PATH", pm)),
    }
}

/// Check if corepack is available and enabled
fn check_corepack_status() -> CheckResult {
    // First check if corepack is available
    match run_command_with_timeout("corepack", &["--version"], SHORT_COMMAND_TIMEOUT) {
        CommandResult::Success(_) => {
            // Corepack is available, check if packageManager field exists in package.json
            if let Ok(pkg_json) = std::fs::read_to_string("package.json") {
                if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&pkg_json) {
                    if pkg.get("packageManager").is_some() {
                        // packageManager field exists, corepack should be used
                        return CheckResult::pass("Corepack enabled", "toolchain");
                    }
                }
            }

            // Corepack available but packageManager not set
            CheckResult::warning(
                "Corepack available",
                "toolchain",
                "Corepack is available but packageManager field not set",
            )
            .with_fix("Add \"packageManager\": \"<pm>@<version>\" to package.json")
        }
        CommandResult::TimedOut => CheckResult::warning(
            "Corepack available",
            "toolchain",
            "Corepack command timed out - skipping corepack check",
        ),
        _ => CheckResult::warning(
            "Corepack available",
            "toolchain",
            "Corepack is not available (comes with Node.js 14.19+)",
        )
        .with_fix("Upgrade to Node.js 14.19+ or run `npm install -g corepack`"),
    }
}

/// Normalize a Node version string for semver parsing
/// Handles suffixes like "-nightly", "-rc.1", "-alpha", etc.
/// Returns the normalized version and whether it had a suffix
fn normalize_node_version(version: &str) -> (String, bool) {
    let version = version.trim().trim_start_matches('v');

    // Check for common suffixes
    let suffixes = [
        "-nightly",
        "-canary",
        "-alpha",
        "-beta",
        "-rc",
        "-pre",
        "-dev",
        "-test",
    ];

    // Find if version contains any suffix at a hyphen boundary
    if let Some(hyphen_idx) = version.find('-') {
        let suffix_part = &version[hyphen_idx..].to_lowercase();

        for suffix in suffixes {
            if suffix_part.starts_with(suffix) {
                // Strip the suffix but keep the base version
                let base = &version[..hyphen_idx];
                return (base.to_string(), true);
            }
        }

        // For other hyphens (might be build metadata or unknown format)
        // Try to extract just the numeric part
        let base = &version[..hyphen_idx];
        if is_valid_semver_base(base) {
            return (base.to_string(), true);
        }
    }

    // Handle plus sign for build metadata
    if let Some(plus_idx) = version.find('+') {
        let base = &version[..plus_idx];
        return (base.to_string(), true);
    }

    (version.to_string(), false)
}

/// Check if a string looks like a valid semver base (X.Y.Z)
fn is_valid_semver_base(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return false;
    }
    parts.iter().all(|p| p.parse::<u32>().is_ok())
}

/// Parse a version string that may have suffixes
fn parse_version_lenient(version: &str) -> Option<semver::Version> {
    let (normalized, _) = normalize_node_version(version);

    // Try parsing directly first
    if let Ok(v) = semver::Version::parse(&normalized) {
        return Some(v);
    }

    // If it's just X.Y format, add .0 for patch
    let parts: Vec<&str> = normalized.split('.').collect();
    if parts.len() == 2 {
        if let Ok(v) = semver::Version::parse(&format!("{}.0", normalized)) {
            return Some(v);
        }
    }

    // If it's just X format, add .0.0
    if parts.len() == 1 {
        if let Ok(v) = semver::Version::parse(&format!("{}.0.0", normalized)) {
            return Some(v);
        }
    }

    None
}

/// Check if current Node version complies with engines field
fn check_engines_compliance(current: &CurrentEnvironment) -> Option<CheckResult> {
    let pkg_json = std::fs::read_to_string("package.json").ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&pkg_json).ok()?;

    let engines = pkg.get("engines")?.as_object()?;
    let node_constraint = engines.get("node")?.as_str()?;

    // Parse current version with lenient parsing for suffixes
    let current_version = match parse_version_lenient(&current.node_version) {
        Some(v) => v,
        None => {
            // If we can't parse it at all, report a warning
            return Some(CheckResult::warning(
                "Engines compliance",
                "toolchain",
                &format!(
                    "Cannot parse Node version '{}' for constraint checking",
                    current.node_version
                ),
            ));
        }
    };

    // Parse constraint (simplified - handles common patterns)
    match check_semver_constraint(node_constraint, &current_version) {
        Some(true) => Some(CheckResult::pass("Engines compliance", "toolchain")),
        Some(false) => Some(
            CheckResult::error(
                "Engines compliance",
                "toolchain",
                &format!(
                    "Node {} does not satisfy engines.node constraint: {}",
                    current.node_version, node_constraint
                ),
            )
            .with_fix(&format!("Install a Node version matching {}", node_constraint)),
        ),
        None => Some(
            CheckResult::warning(
                "Engines compliance",
                "toolchain",
                &format!(
                    "Unrecognized constraint format '{}', skipping check",
                    node_constraint
                ),
            )
        ),
    }
}

/// Simple semver constraint checker
/// Returns Some(true) if satisfied, Some(false) if not, None if constraint format unrecognized
fn check_semver_constraint(constraint: &str, version: &semver::Version) -> Option<bool> {
    let constraint = constraint.trim();

    // Handle common patterns
    if constraint.starts_with(">=") {
        if let Some(min) = parse_version_lenient(constraint.trim_start_matches(">=").trim()) {
            return Some(version >= &min);
        }
    } else if constraint.starts_with('>') {
        if let Some(min) = parse_version_lenient(constraint.trim_start_matches('>').trim()) {
            return Some(version > &min);
        }
    } else if constraint.starts_with("<=") {
        if let Some(max) = parse_version_lenient(constraint.trim_start_matches("<=").trim()) {
            return Some(version <= &max);
        }
    } else if constraint.starts_with('<') {
        if let Some(max) = parse_version_lenient(constraint.trim_start_matches('<').trim()) {
            return Some(version < &max);
        }
    } else if constraint.starts_with('^') {
        // Caret: allows minor and patch updates
        let base = constraint.trim_start_matches('^').trim();
        if let Some(base_ver) = parse_version_lenient(base) {
            return Some(version.major == base_ver.major && version >= &base_ver);
        }
    } else if constraint.starts_with('~') {
        // Tilde: allows patch updates
        let base = constraint.trim_start_matches('~').trim();
        if let Some(base_ver) = parse_version_lenient(base) {
            return Some(
                version.major == base_ver.major
                    && version.minor == base_ver.minor
                    && version >= &base_ver,
            );
        }
    } else if constraint.contains("||") {
        // OR operator - if any part is satisfied, return true
        // If all parts are unrecognized, return None
        let results: Vec<Option<bool>> = constraint
            .split("||")
            .map(|c| check_semver_constraint(c.trim(), version))
            .collect();

        if results.iter().any(|r| *r == Some(true)) {
            return Some(true);
        }
        if results.iter().all(|r| r.is_none()) {
            return None;
        }
        return Some(false);
    } else if constraint.contains(' ') {
        // AND operator (space-separated) - all parts must be satisfied
        let results: Vec<Option<bool>> = constraint
            .split_whitespace()
            .map(|c| check_semver_constraint(c, version))
            .collect();

        if results.iter().any(|r| r.is_none()) {
            return None;
        }
        return Some(results.iter().all(|r| *r == Some(true)));
    } else if constraint == "*" || constraint == "x" || constraint == "X" {
        // Wildcard - any version matches
        return Some(true);
    } else if let Some(exact) = parse_version_lenient(constraint) {
        return Some(version == &exact);
    }

    // Unrecognized constraint format
    None
}
