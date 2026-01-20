pub mod toolchain;
pub mod lockfile_checks;
pub mod dependencies;
pub mod frameworks;

use anyhow::Result;
use clap::ValueEnum;
use serde::Serialize;
use std::path::Path;

use crate::config::ZenvoConfig;
use crate::lockfile::EnvLock;

/// Valid check categories for the doctor command
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CheckCategory {
    /// Toolchain checks (Node.js, package manager versions)
    Toolchain,
    /// Lockfile checks (existence, integrity, sync)
    Lockfile,
    /// Dependency checks (node_modules, peer deps)
    Deps,
    /// Framework checks (React, Next.js, TypeScript)
    Frameworks,
}


/// Result of checking for package.json
#[derive(Debug)]
pub enum PackageJsonStatus {
    /// package.json exists and is valid
    Valid(serde_json::Value),
    /// package.json exists but is invalid JSON
    Invalid(String),
    /// package.json does not exist
    Missing,
    /// Cannot read package.json (permissions or other error)
    Unreadable(String),
}

/// Check for package.json and return its status
pub fn check_package_json() -> PackageJsonStatus {
    let path = Path::new("package.json");

    if !path.exists() {
        return PackageJsonStatus::Missing;
    }

    match std::fs::read_to_string(path) {
        Ok(content) => {
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(pkg) => PackageJsonStatus::Valid(pkg),
                Err(e) => PackageJsonStatus::Invalid(e.to_string()),
            }
        }
        Err(e) => {
            // Provide more specific error messages based on the error kind
            let msg = match e.kind() {
                std::io::ErrorKind::PermissionDenied => {
                    "Permission denied - check file permissions".to_string()
                }
                std::io::ErrorKind::NotFound => {
                    "File not found".to_string()
                }
                _ => format!("Cannot read file: {}", e),
            };
            PackageJsonStatus::Unreadable(msg)
        }
    }
}

/// Check if running in a monorepo/workspace context
pub fn detect_workspace_root() -> Option<WorkspaceInfo> {
    let pkg_status = check_package_json();

    if let PackageJsonStatus::Valid(pkg) = pkg_status {
        // Check for npm/yarn workspaces
        if let Some(workspaces) = pkg.get("workspaces") {
            let packages = if let Some(arr) = workspaces.as_array() {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            } else if let Some(obj) = workspaces.as_object() {
                // Yarn workspace format: { "packages": [...] }
                obj.get("packages")
                    .and_then(|p| p.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            if !packages.is_empty() {
                return Some(WorkspaceInfo {
                    workspace_type: WorkspaceType::NpmYarn,
                    packages,
                });
            }
        }
    }

    // Check for pnpm workspaces
    if Path::new("pnpm-workspace.yaml").exists() {
        if let Ok(content) = std::fs::read_to_string("pnpm-workspace.yaml") {
            if let Ok(workspace) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                if let Some(packages) = workspace.get("packages").and_then(|p| p.as_sequence()) {
                    let pkg_list: Vec<String> = packages
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect();

                    if !pkg_list.is_empty() {
                        return Some(WorkspaceInfo {
                            workspace_type: WorkspaceType::Pnpm,
                            packages: pkg_list,
                        });
                    }
                }
            }
        }
    }

    // Check for Nx monorepo
    if Path::new("nx.json").exists() {
        return Some(WorkspaceInfo {
            workspace_type: WorkspaceType::Nx,
            packages: Vec::new(), // Nx has different project structure
        });
    }

    // Check for Turborepo
    if Path::new("turbo.json").exists() {
        return Some(WorkspaceInfo {
            workspace_type: WorkspaceType::Turbo,
            packages: Vec::new(),
        });
    }

    // Check for Lerna
    if Path::new("lerna.json").exists() {
        return Some(WorkspaceInfo {
            workspace_type: WorkspaceType::Lerna,
            packages: Vec::new(),
        });
    }

    None
}

/// Type of workspace/monorepo
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceType {
    NpmYarn,
    Pnpm,
    Nx,
    Turbo,
    Lerna,
}

impl std::fmt::Display for WorkspaceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceType::NpmYarn => write!(f, "npm/yarn workspaces"),
            WorkspaceType::Pnpm => write!(f, "pnpm workspaces"),
            WorkspaceType::Nx => write!(f, "Nx monorepo"),
            WorkspaceType::Turbo => write!(f, "Turborepo"),
            WorkspaceType::Lerna => write!(f, "Lerna"),
        }
    }
}

/// Information about detected workspace
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub workspace_type: WorkspaceType,
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum CheckSeverity {
    Pass,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub category: String,
    pub severity: CheckSeverity,
    pub message: String,
    pub suggested_fix: Option<String>,
}

impl CheckResult {
    pub fn pass(name: &str, category: &str) -> Self {
        Self {
            name: name.to_string(),
            category: category.to_string(),
            severity: CheckSeverity::Pass,
            message: String::new(),
            suggested_fix: None,
        }
    }

    pub fn error(name: &str, category: &str, message: &str) -> Self {
        Self {
            name: name.to_string(),
            category: category.to_string(),
            severity: CheckSeverity::Error,
            message: message.to_string(),
            suggested_fix: None,
        }
    }

    pub fn warning(name: &str, category: &str, message: &str) -> Self {
        Self {
            name: name.to_string(),
            category: category.to_string(),
            severity: CheckSeverity::Warning,
            message: message.to_string(),
            suggested_fix: None,
        }
    }

    pub fn info(name: &str, category: &str, message: &str) -> Self {
        Self {
            name: name.to_string(),
            category: category.to_string(),
            severity: CheckSeverity::Info,
            message: message.to_string(),
            suggested_fix: None,
        }
    }

    pub fn with_fix(mut self, fix: &str) -> Self {
        self.suggested_fix = Some(fix.to_string());
        self
    }
}

#[derive(Debug, Clone)]
pub struct CurrentEnvironment {
    pub node_version: String,
    pub package_manager: String,
    pub package_manager_version: String,
    pub lockfile_type: Option<String>,
    pub lockfile_hash: Option<String>,
}

pub fn detect_current_environment() -> Result<CurrentEnvironment> {
    let node_version = toolchain::detect_node_version()?;
    let (pm, pm_version) = toolchain::detect_package_manager()?;
    let (lockfile_type, lockfile_hash) = lockfile_checks::detect_lockfile()?;

    Ok(CurrentEnvironment {
        node_version,
        package_manager: pm,
        package_manager_version: pm_version,
        lockfile_type,
        lockfile_hash,
    })
}

pub fn run_all_checks(
    env_lock: &Option<EnvLock>,
    category: Option<CheckCategory>,
    config: &Option<ZenvoConfig>,
) -> Result<Vec<CheckResult>> {
    let mut results = Vec::new();

    // Early check: Verify package.json status before running other checks
    let pkg_status = check_package_json();
    match &pkg_status {
        PackageJsonStatus::Missing => {
            results.push(
                CheckResult::error(
                    "package.json exists",
                    "project",
                    "No package.json found in current directory",
                )
                .with_fix("Run `npm init` or `yarn init` to create package.json"),
            );
        }
        PackageJsonStatus::Invalid(err) => {
            results.push(
                CheckResult::error(
                    "package.json valid",
                    "project",
                    &format!("package.json is invalid JSON: {}", err),
                )
                .with_fix("Fix the JSON syntax in package.json"),
            );
        }
        PackageJsonStatus::Unreadable(err) => {
            results.push(
                CheckResult::error(
                    "package.json readable",
                    "project",
                    &format!("Cannot read package.json: {}", err),
                )
                .with_fix("Check file permissions: chmod 644 package.json"),
            );
        }
        PackageJsonStatus::Valid(_) => {
            results.push(CheckResult::pass("package.json valid", "project"));
        }
    }

    // Check for workspace/monorepo
    if let Some(workspace) = detect_workspace_root() {
        results.push(CheckResult::info(
            "Workspace detected",
            "project",
            &format!(
                "Running in {} context{}",
                workspace.workspace_type,
                if workspace.packages.is_empty() {
                    String::new()
                } else {
                    format!(" ({} packages)", workspace.packages.len())
                }
            ),
        ));
    }

    let current = detect_current_environment()?;

    // Filter by category if specified
    let run_toolchain = category.is_none() || category == Some(CheckCategory::Toolchain);
    let run_lockfile = category.is_none() || category == Some(CheckCategory::Lockfile);
    let run_deps = category.is_none() || category == Some(CheckCategory::Deps);
    let run_frameworks = category.is_none() || category == Some(CheckCategory::Frameworks);

    // Toolchain checks
    if run_toolchain {
        results.extend(toolchain::run_checks(&current, env_lock)?);
    }

    // Lockfile checks
    if run_lockfile {
        results.extend(lockfile_checks::run_checks(&current, env_lock)?);
    }

    // Dependency checks
    if run_deps {
        results.extend(dependencies::run_checks()?);
    }

    // Framework checks
    if run_frameworks {
        results.extend(frameworks::run_checks()?);
    }

    // Apply config (filter disabled checks, apply severity overrides)
    if let Some(cfg) = config {
        results = apply_config_to_results(results, cfg);
    }

    Ok(results)
}

/// Apply configuration settings to check results
/// - Filters out disabled checks
/// - Applies severity overrides
fn apply_config_to_results(results: Vec<CheckResult>, config: &ZenvoConfig) -> Vec<CheckResult> {
    results
        .into_iter()
        .filter(|r| !config.is_check_disabled(&r.name))
        .map(|mut r| {
            if let Some(severity) = config.get_severity_override(&r.name) {
                r.severity = severity;
            }
            r
        })
        .collect()
}
