use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

use crate::checks::{lockfile_checks, toolchain};
use crate::utils::{run_command_with_timeout, CommandResult, SHORT_COMMAND_TIMEOUT};

/// Current schema version for env.lock files
pub const CURRENT_SCHEMA_VERSION: &str = "1.0";

/// Minimum schema version that this version of Zenvo can read
pub const MIN_SUPPORTED_SCHEMA_VERSION: &str = "1.0";

/// Schema version validation result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaVersionStatus {
    /// Version is current and fully supported
    Current,
    /// Version is older but still supported (may lack some features)
    Supported { version: String },
    /// Version is too old and not supported
    TooOld { version: String, minimum: String },
    /// Version is newer than this tool supports
    TooNew { version: String, current: String },
    /// Version is missing or invalid
    Invalid { reason: String },
}

impl SchemaVersionStatus {
    /// Returns true if the schema can be loaded
    #[allow(dead_code)]
    pub fn is_loadable(&self) -> bool {
        matches!(self, SchemaVersionStatus::Current | SchemaVersionStatus::Supported { .. })
    }
}

/// Validate a schema version string
fn validate_schema_version(version: &str) -> SchemaVersionStatus {
    // Parse versions as (major, minor)
    let parse_version = |v: &str| -> Option<(u32, u32)> {
        let parts: Vec<&str> = v.split('.').collect();
        if parts.len() != 2 {
            return None;
        }
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        Some((major, minor))
    };

    let current = match parse_version(CURRENT_SCHEMA_VERSION) {
        Some(v) => v,
        None => return SchemaVersionStatus::Invalid {
            reason: "Internal error: invalid current schema version".to_string(),
        },
    };

    let minimum = match parse_version(MIN_SUPPORTED_SCHEMA_VERSION) {
        Some(v) => v,
        None => return SchemaVersionStatus::Invalid {
            reason: "Internal error: invalid minimum schema version".to_string(),
        },
    };

    let file_version = match parse_version(version) {
        Some(v) => v,
        None => return SchemaVersionStatus::Invalid {
            reason: format!("Invalid schema version format: '{}' (expected X.Y)", version),
        },
    };

    // Check if version is too new
    if file_version.0 > current.0 {
        return SchemaVersionStatus::TooNew {
            version: version.to_string(),
            current: CURRENT_SCHEMA_VERSION.to_string(),
        };
    }

    // Check if version is too old
    if file_version < minimum {
        return SchemaVersionStatus::TooOld {
            version: version.to_string(),
            minimum: MIN_SUPPORTED_SCHEMA_VERSION.to_string(),
        };
    }

    // Check if version is current
    if file_version == current {
        SchemaVersionStatus::Current
    } else {
        SchemaVersionStatus::Supported {
            version: version.to_string(),
        }
    }
}

/// The main env.lock structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvLock {
    pub metadata: Metadata,
    pub toolchain: Toolchain,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lockfile: Option<LockfileInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caches: Option<Caches>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frameworks: Option<Frameworks>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub version: String,
    pub generated_at: String,
    pub generated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Toolchain {
    pub node: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_version_source: Option<String>,
    pub package_manager: String,
    pub package_manager_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corepack_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockfileInfo {
    #[serde(rename = "type")]
    pub lockfile_type: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Caches {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_modules_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnpm_store_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frameworks {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub react: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typescript: Option<String>,
}

impl EnvLock {
    /// Generate a new env.lock from the current environment
    pub fn generate() -> Result<Self> {
        let (node_version, version_manager) = toolchain::detect_node_version_with_source()
            .context("Failed to detect Node.js version")?;

        let (pm, pm_version) = toolchain::detect_package_manager()
            .context("Failed to detect package manager")?;

        let (lockfile_type, lockfile_hash) = lockfile_checks::detect_lockfile()?;

        let lockfile = lockfile_type.map(|t| LockfileInfo {
            lockfile_type: t,
            hash: lockfile_hash.unwrap_or_default(),
        });

        let frameworks = detect_frameworks()?;
        let caches = detect_caches(&pm);
        let corepack_enabled = toolchain::detect_corepack_enabled();

        // Convert version manager to string for storage
        let node_version_source = match version_manager {
            toolchain::NodeVersionManager::Unknown => None,
            _ => Some(version_manager.to_string()),
        };

        Ok(Self {
            metadata: Metadata {
                version: CURRENT_SCHEMA_VERSION.to_string(),
                generated_at: Utc::now().to_rfc3339(),
                generated_by: format!("zenvo@{}", env!("CARGO_PKG_VERSION")),
            },
            toolchain: Toolchain {
                node: node_version,
                node_version_source,
                package_manager: pm,
                package_manager_version: pm_version,
                corepack_enabled,
            },
            environment: None,
            lockfile,
            caches,
            frameworks,
        })
    }

    /// Include system info (OS, arch)
    pub fn include_system_info(&mut self) -> Result<()> {
        self.environment = Some(Environment {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        });
        Ok(())
    }

    /// Save to file (TOML format)
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize env.lock")?;
        
        let header = "# env.lock - Generated by Zenvo\n# DO NOT EDIT MANUALLY - Regenerate with `zenvo lock`\n\n";
        let full_content = format!("{}{}", header, content);
        
        fs::write(path, full_content)
            .context("Failed to write env.lock")?;
        
        Ok(())
    }

    /// Load from file with schema version validation
    pub fn load() -> Result<Self> {
        let path = Path::new("env.lock");
        if !path.exists() {
            anyhow::bail!("env.lock not found. Run `zenvo init` to create one.");
        }

        let content = fs::read_to_string(path)
            .context("Failed to read env.lock")?;

        let env_lock: EnvLock = toml::from_str(&content)
            .context("Failed to parse env.lock")?;

        // Validate schema version
        env_lock.validate_schema()?;

        Ok(env_lock)
    }

    /// Load if exists with schema version validation, otherwise return None
    pub fn load_if_exists() -> Result<Option<Self>> {
        let path = Path::new("env.lock");
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(path)?;
        let env_lock: EnvLock = toml::from_str(&content)?;

        // Validate schema version
        env_lock.validate_schema()?;

        Ok(Some(env_lock))
    }

    /// Validate the schema version of this env.lock
    pub fn validate_schema(&self) -> Result<SchemaVersionStatus> {
        let status = validate_schema_version(&self.metadata.version);

        match &status {
            SchemaVersionStatus::Current => Ok(status),
            SchemaVersionStatus::Supported { version } => {
                // Log a warning but allow loading
                eprintln!(
                    "Warning: env.lock uses schema version {} (current is {}). \
                     Consider running `zenvo lock` to update.",
                    version, CURRENT_SCHEMA_VERSION
                );
                Ok(status)
            }
            SchemaVersionStatus::TooOld { version, minimum } => {
                anyhow::bail!(
                    "env.lock schema version {} is too old (minimum supported: {}). \
                     Run `zenvo lock --force` to regenerate.",
                    version, minimum
                )
            }
            SchemaVersionStatus::TooNew { version, current } => {
                anyhow::bail!(
                    "env.lock schema version {} is newer than this version of Zenvo supports ({}). \
                     Please upgrade Zenvo: `cargo install zenvo` or `npm install -g zenvo`",
                    version, current
                )
            }
            SchemaVersionStatus::Invalid { reason } => {
                anyhow::bail!("Invalid env.lock schema version: {}", reason)
            }
        }
    }

    /// Get the schema version status without failing
    #[allow(dead_code)]
    pub fn schema_status(&self) -> SchemaVersionStatus {
        validate_schema_version(&self.metadata.version)
    }
}

fn detect_frameworks() -> Result<Option<Frameworks>> {
    let pkg_json = match fs::read_to_string("package.json") {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let pkg: serde_json::Value = match serde_json::from_str(&pkg_json) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let deps = pkg.get("dependencies").and_then(|d| d.as_object());
    let dev_deps = pkg.get("devDependencies").and_then(|d| d.as_object());

    let get_version = |name: &str| -> Option<String> {
        deps.and_then(|d| d.get(name))
            .or_else(|| dev_deps.and_then(|d| d.get(name)))
            .and_then(|v| v.as_str())
            .map(|s| s.trim_start_matches('^').trim_start_matches('~').to_string())
    };

    let react = get_version("react");
    let next = get_version("next");
    let typescript = get_version("typescript");

    if react.is_none() && next.is_none() && typescript.is_none() {
        return Ok(None);
    }

    Ok(Some(Frameworks {
        react,
        next,
        typescript,
    }))
}

/// Detect cache information
fn detect_caches(package_manager: &str) -> Option<Caches> {
    let node_modules_hash = compute_node_modules_hash();
    let pnpm_store_path = if package_manager == "pnpm" {
        get_pnpm_store_path()
    } else {
        None
    };

    if node_modules_hash.is_none() && pnpm_store_path.is_none() {
        return None;
    }

    Some(Caches {
        node_modules_hash,
        pnpm_store_path,
    })
}

/// Compute a hash of the node_modules directory
/// Uses package names and versions from top-level dependencies (max_depth=2)
/// Handles symlinks (common in pnpm) by following them to read package.json
fn compute_node_modules_hash() -> Option<String> {
    let node_modules = Path::new("node_modules");
    if !node_modules.exists() {
        return None;
    }

    let mut hasher = Sha256::new();
    let mut packages: Vec<String> = Vec::new();

    // Detect pnpm structure (has .pnpm directory)
    let is_pnpm = node_modules.join(".pnpm").exists();

    // Read top-level packages in node_modules
    if let Ok(entries) = fs::read_dir(node_modules) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files/folders (but not .bin which is valid)
            if name.starts_with('.') && name != ".bin" {
                continue;
            }

            // Skip .bin directory for package enumeration
            if name == ".bin" {
                continue;
            }

            // Handle scoped packages (@org/package)
            if name.starts_with('@') {
                if let Ok(scoped_entries) = fs::read_dir(&path) {
                    for scoped_entry in scoped_entries.filter_map(|e| e.ok()) {
                        let scoped_path = scoped_entry.path();
                        let scoped_name = format!("{}/{}", name, scoped_entry.file_name().to_string_lossy());

                        // Resolve symlinks (common in pnpm)
                        let resolved_path = resolve_symlink_if_needed(&scoped_path);
                        if let Some(version) = get_package_version(&resolved_path) {
                            packages.push(format!("{}@{}", scoped_name, version));
                        }
                    }
                }
            } else {
                // Resolve symlinks (common in pnpm)
                let resolved_path = resolve_symlink_if_needed(&path);
                if let Some(version) = get_package_version(&resolved_path) {
                    packages.push(format!("{}@{}", name, version));
                }
            }
        }
    }

    // For pnpm, also enumerate packages from .pnpm directory for completeness
    if is_pnpm {
        if let Some(pnpm_packages) = enumerate_pnpm_store_packages(&node_modules.join(".pnpm")) {
            for pkg in pnpm_packages {
                if !packages.contains(&pkg) {
                    packages.push(pkg);
                }
            }
        }
    }

    if packages.is_empty() {
        return None;
    }

    // Sort for deterministic hash
    packages.sort();

    for pkg in &packages {
        hasher.update(pkg.as_bytes());
        hasher.update(b"\n");
    }

    let result = hasher.finalize();
    Some(format!("sha256:{:x}", result))
}

/// Resolve symlink if the path is a symlink, otherwise return the original path
fn resolve_symlink_if_needed(path: &Path) -> std::path::PathBuf {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                // Resolve the symlink
                match fs::read_link(path) {
                    Ok(target) => {
                        // If target is relative, resolve it relative to the symlink's parent
                        if target.is_relative() {
                            if let Some(parent) = path.parent() {
                                return parent.join(&target);
                            }
                        }
                        target
                    }
                    Err(_) => path.to_path_buf(),
                }
            } else {
                path.to_path_buf()
            }
        }
        Err(_) => path.to_path_buf(),
    }
}

/// Enumerate packages from pnpm's .pnpm store directory
/// The structure is: .pnpm/<package-name>@<version>/node_modules/<package-name>
fn enumerate_pnpm_store_packages(pnpm_dir: &Path) -> Option<Vec<String>> {
    if !pnpm_dir.exists() {
        return None;
    }

    let mut packages = Vec::new();

    if let Ok(entries) = fs::read_dir(pnpm_dir) {
        for entry in entries.filter_map(|e| e.ok()).take(1000) {
            // Limit to first 1000 entries
            let dir_name = entry.file_name().to_string_lossy().to_string();

            // pnpm format: package-name@version or @scope+package-name@version
            if let Some((name, version)) = parse_pnpm_package_dir(&dir_name) {
                packages.push(format!("{}@{}", name, version));
            }
        }
    }

    if packages.is_empty() {
        None
    } else {
        Some(packages)
    }
}

/// Parse pnpm package directory name into (name, version)
/// Formats: "lodash@4.17.21" or "@types+node@18.0.0"
fn parse_pnpm_package_dir(dir_name: &str) -> Option<(String, String)> {
    // Skip directories that don't look like package directories
    if dir_name.starts_with('.') || !dir_name.contains('@') {
        return None;
    }

    // Handle scoped packages (@ in pnpm becomes +)
    if dir_name.starts_with('@') || dir_name.contains('+') {
        // Scoped package: @scope+name@version
        // Find the last @ which separates name from version
        if let Some(last_at) = dir_name.rfind('@') {
            if last_at > 0 {
                let name_part = &dir_name[..last_at];
                let version = &dir_name[last_at + 1..];

                // Convert + back to / for scoped packages
                let name = name_part.replace('+', "/");

                // Validate version (should not be empty and should look like a version)
                if !version.is_empty() && version.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    return Some((name, version.to_string()));
                }
            }
        }
    } else {
        // Regular package: name@version
        if let Some(at_idx) = dir_name.rfind('@') {
            if at_idx > 0 {
                let name = &dir_name[..at_idx];
                let version = &dir_name[at_idx + 1..];

                if !version.is_empty() && version.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    return Some((name.to_string(), version.to_string()));
                }
            }
        }
    }

    None
}

/// Get package version from its package.json
fn get_package_version(package_path: &Path) -> Option<String> {
    let pkg_json_path = package_path.join("package.json");
    let content = fs::read_to_string(pkg_json_path).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;
    pkg.get("version")?.as_str().map(|s| s.to_string())
}

/// Get pnpm store path
fn get_pnpm_store_path() -> Option<String> {
    match run_command_with_timeout("pnpm", &["store", "path"], SHORT_COMMAND_TIMEOUT) {
        CommandResult::Success(output) => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                Some(path)
            } else {
                None
            }
        }
        _ => None,
    }
}
