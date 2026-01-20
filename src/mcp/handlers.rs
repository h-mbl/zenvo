//! MCP tool handlers for Zenvo

use anyhow::Result;
use serde_json::Value;
use std::env;
use std::path::Path;

use crate::checks::{detect_current_environment, run_all_checks, CheckCategory, CheckSeverity};
use crate::config::ZenvoConfig;
use crate::lockfile::EnvLock;
use crate::output::Issue;
use crate::repair::{generate_repair_plan_with_context, RepairContext};

/// Change to specified directory if path is provided
fn with_directory<T, F>(args: &Value, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let path = args.get("path").and_then(|v| v.as_str());

    if let Some(dir) = path {
        let original_dir = env::current_dir()?;
        env::set_current_dir(dir)?;
        let result = f();
        // Restore original directory
        env::set_current_dir(original_dir)?;
        result
    } else {
        f()
    }
}

/// Detect Node.js subdirectories (containing package.json)
pub fn detect_node_projects(_args: &Value) -> Result<Value> {
    let mut projects = Vec::new();

    // Check current directory
    if Path::new("package.json").exists() {
        projects.push(serde_json::json!({
            "path": ".",
            "name": get_package_name("package.json")
        }));
    }

    // Check common subdirectories
    for subdir in &["frontend", "client", "web", "app", "packages", "apps"] {
        let pkg_path = format!("{}/package.json", subdir);
        if Path::new(&pkg_path).exists() {
            projects.push(serde_json::json!({
                "path": subdir,
                "name": get_package_name(&pkg_path)
            }));
        }
    }

    Ok(serde_json::json!({
        "projects": projects,
        "hint": if projects.is_empty() {
            "No Node.js projects found. Make sure package.json exists."
        } else if projects.len() == 1 {
            "Found 1 Node.js project"
        } else {
            "Multiple Node.js projects found. Use 'path' parameter to specify which one."
        }
    }))
}

fn get_package_name(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .and_then(|pkg| pkg.get("name").and_then(|n| n.as_str()).map(String::from))
}

/// Get the current environment status
pub fn get_environment_status(args: &Value) -> Result<Value> {
    with_directory(args, get_environment_status_impl)
}

fn get_environment_status_impl() -> Result<Value> {
    // Detect current environment
    let current = detect_current_environment()?;

    // Load env.lock if it exists
    let locked = EnvLock::load_if_exists()?;

    // Load config if it exists
    let config = ZenvoConfig::load_if_exists()?;

    // Run all checks
    let results = run_all_checks(&locked, None, &config)?;

    // Convert issues
    let issues: Vec<Issue> = results
        .iter()
        .filter(|r| r.severity != CheckSeverity::Pass)
        .map(Issue::from)
        .collect();

    // Check for drift
    let has_drift = if let Some(ref lock) = locked {
        current.node_version != lock.toolchain.node
            || current.package_manager != lock.toolchain.package_manager
            || current.package_manager_version != lock.toolchain.package_manager_version
    } else {
        false
    };

    // Build response
    let mut response = serde_json::json!({
        "current": {
            "node_version": current.node_version,
            "package_manager": current.package_manager,
            "package_manager_version": current.package_manager_version,
            "lockfile_type": current.lockfile_type,
            "lockfile_hash": current.lockfile_hash
        },
        "has_env_lock": locked.is_some(),
        "drift_detected": has_drift,
        "issues": issues,
        "summary": {
            "total_checks": results.len(),
            "passed": results.iter().filter(|r| r.severity == CheckSeverity::Pass).count(),
            "warnings": results.iter().filter(|r| r.severity == CheckSeverity::Warning).count(),
            "errors": results.iter().filter(|r| r.severity == CheckSeverity::Error).count()
        }
    });

    if let Some(ref lock) = locked {
        response["locked"] = serde_json::json!({
            "node": lock.toolchain.node,
            "package_manager": lock.toolchain.package_manager,
            "package_manager_version": lock.toolchain.package_manager_version
        });
    }

    Ok(response)
}

/// Sync environment - update env.lock
pub fn sync_environment(args: &Value) -> Result<Value> {
    with_directory(args, || sync_environment_impl(args))
}

fn sync_environment_impl(args: &Value) -> Result<Value> {
    let include_system_info = args
        .get("include_system_info")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut env_lock = EnvLock::generate()?;

    if include_system_info {
        env_lock.include_system_info()?;
    }

    env_lock.save(Path::new("env.lock"))?;

    Ok(serde_json::json!({
        "success": true,
        "message": "env.lock updated successfully",
        "path": env::current_dir()?.to_string_lossy(),
        "toolchain": {
            "node": env_lock.toolchain.node,
            "package_manager": env_lock.toolchain.package_manager,
            "package_manager_version": env_lock.toolchain.package_manager_version
        }
    }))
}

/// Fix drift - generate and optionally execute repair plan
pub fn fix_drift(args: &Value) -> Result<Value> {
    with_directory(args, || fix_drift_impl(args))
}

fn fix_drift_impl(args: &Value) -> Result<Value> {
    let execute = args
        .get("execute")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let safe_only = args
        .get("safe_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // Load env.lock
    let env_lock = EnvLock::load()?;

    // Load config if it exists
    let config = ZenvoConfig::load_if_exists()?;

    // Run checks to find issues
    let results = run_all_checks(&Some(env_lock.clone()), None, &config)?;
    let issues: Vec<_> = results
        .iter()
        .filter(|r| r.severity == CheckSeverity::Error || r.severity == CheckSeverity::Warning)
        .collect();

    if issues.is_empty() {
        return Ok(serde_json::json!({
            "success": true,
            "message": "No issues to repair - environment is healthy",
            "actions": []
        }));
    }

    // Create repair context from env.lock
    let repair_context = RepairContext::new(&env_lock.toolchain.package_manager)
        .with_node_version_manager(env_lock.toolchain.node_version_source.clone())
        .with_target_node_version(Some(env_lock.toolchain.node.clone()));

    // Generate repair plan with context
    let repair_plan = generate_repair_plan_with_context(&issues, &repair_context)?;

    // Build actions list
    let actions: Vec<Value> = repair_plan
        .iter()
        .map(|a| {
            serde_json::json!({
                "description": a.description,
                "command": a.command,
                "is_safe": a.is_safe
            })
        })
        .collect();

    if !execute {
        return Ok(serde_json::json!({
            "success": true,
            "message": "Repair plan generated (not executed)",
            "total_issues": issues.len(),
            "actions": actions
        }));
    }

    // Execute repairs
    let mut executed = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();

    for action in &repair_plan {
        // Skip non-safe actions if safe_only
        if safe_only && !action.is_safe {
            skipped.push(action.description.clone());
            continue;
        }

        match crate::repair::execute_repair(action) {
            Ok(_) => executed.push(action.description.clone()),
            Err(e) => failed.push(serde_json::json!({
                "action": action.description,
                "error": e.to_string()
            })),
        }
    }

    Ok(serde_json::json!({
        "success": failed.is_empty(),
        "message": if failed.is_empty() {
            "Repair completed successfully"
        } else {
            "Repair completed with some failures"
        },
        "executed": executed,
        "skipped": skipped,
        "failed": failed
    }))
}

/// Run doctor checks
pub fn run_doctor(args: &Value) -> Result<Value> {
    with_directory(args, || run_doctor_impl(args))
}

fn run_doctor_impl(args: &Value) -> Result<Value> {
    // Parse category from string to enum
    let category = args
        .get("category")
        .and_then(|v| v.as_str())
        .and_then(|s| match s.to_lowercase().as_str() {
            "toolchain" => Some(CheckCategory::Toolchain),
            "lockfile" => Some(CheckCategory::Lockfile),
            "deps" => Some(CheckCategory::Deps),
            "frameworks" => Some(CheckCategory::Frameworks),
            _ => None,
        });

    // Load env.lock if it exists
    let env_lock = EnvLock::load_if_exists()?;

    // Load config if it exists
    let config = ZenvoConfig::load_if_exists()?;

    // Run checks
    let results = run_all_checks(&env_lock, category, &config)?;

    // Convert to issues
    let issues: Vec<Issue> = results.iter().map(Issue::from).collect();

    let errors = results
        .iter()
        .filter(|r| r.severity == CheckSeverity::Error)
        .count();
    let warnings = results
        .iter()
        .filter(|r| r.severity == CheckSeverity::Warning)
        .count();
    let passed = results
        .iter()
        .filter(|r| r.severity == CheckSeverity::Pass)
        .count();

    Ok(serde_json::json!({
        "success": errors == 0,
        "drift_detected": errors > 0 || warnings > 0,
        "issues": issues,
        "summary": {
            "total": results.len(),
            "passed": passed,
            "warnings": warnings,
            "errors": errors
        }
    }))
}

/// Search for available package versions on npm registry
pub fn search_versions(args: &Value) -> Result<Value> {
    let package = args
        .get("package")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: package"))?;

    let constraint = args.get("constraint").and_then(|v| v.as_str());

    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    // Fetch from npm registry
    let encoded_package = package.replace("/", "%2f");
    let url = format!("https://registry.npmjs.org/{}", encoded_package);

    let response = reqwest::blocking::Client::new()
        .get(&url)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .map_err(|e| anyhow::anyhow!("Failed to connect to npm registry: {}", e))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(serde_json::json!({
            "success": false,
            "error": format!("Package '{}' not found on npm registry", package),
            "package": package,
            "versions": []
        }));
    }

    if !response.status().is_success() {
        anyhow::bail!("npm registry returned error: {}", response.status());
    }

    let info: serde_json::Value = response.json()?;

    // Get dist-tags
    let dist_tags = info.get("dist-tags").cloned();
    let latest = dist_tags
        .as_ref()
        .and_then(|t| t.get("latest"))
        .and_then(|v| v.as_str());

    // Get versions
    let versions_obj = info
        .get("versions")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow::anyhow!("No versions found"))?;

    let time_obj = info.get("time").and_then(|v| v.as_object());

    // Build version list
    let mut versions: Vec<serde_json::Value> = versions_obj
        .keys()
        .map(|version| {
            let is_deprecated = versions_obj
                .get(version)
                .and_then(|v| v.get("deprecated"))
                .map(|d| !d.is_null())
                .unwrap_or(false);

            let published = time_obj
                .and_then(|t| t.get(version))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            serde_json::json!({
                "version": version,
                "published": published,
                "latest": latest == Some(version.as_str()),
                "deprecated": is_deprecated
            })
        })
        .collect();

    // Sort by version (newest first) - simple string comparison for now
    versions.sort_by(|a, b| {
        let va = a.get("version").and_then(|v| v.as_str()).unwrap_or("");
        let vb = b.get("version").and_then(|v| v.as_str()).unwrap_or("");
        compare_versions(vb, va)
    });

    // Filter by constraint if provided
    if let Some(constraint_str) = constraint {
        versions = versions
            .into_iter()
            .filter(|v| {
                let version = v.get("version").and_then(|v| v.as_str()).unwrap_or("");
                matches_version_constraint(version, constraint_str)
            })
            .collect();
    }

    // Limit results
    let display_versions: Vec<_> = versions.into_iter().take(limit).collect();

    // Suggest best version
    let suggestion = if !display_versions.is_empty() {
        let best = display_versions
            .iter()
            .find(|v| !v.get("deprecated").and_then(|d| d.as_bool()).unwrap_or(false))
            .or(display_versions.first());

        best.and_then(|v| v.get("version"))
            .and_then(|v| v.as_str())
            .map(|v| format!("{}@{}", package, v))
    } else {
        None
    };

    Ok(serde_json::json!({
        "success": true,
        "package": package,
        "constraint": constraint,
        "dist_tags": dist_tags,
        "versions": display_versions,
        "suggestion": suggestion,
        "hint": if display_versions.is_empty() && constraint.is_some() {
            Some(format!("No versions match constraint '{}'. Try a different constraint.", constraint.unwrap()))
        } else {
            None
        }
    }))
}

/// Simple version comparison for sorting
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split('-')
            .next()
            .unwrap_or("")
            .split('.')
            .filter_map(|p| p.parse().ok())
            .collect()
    };

    let va = parse(a);
    let vb = parse(b);

    for i in 0..3 {
        let a_part = va.get(i).copied().unwrap_or(0);
        let b_part = vb.get(i).copied().unwrap_or(0);
        match a_part.cmp(&b_part) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// Check if version matches constraint (simplified)
fn matches_version_constraint(version: &str, constraint: &str) -> bool {
    let constraint = constraint.trim();

    let (operator, target) = if constraint.starts_with(">=") {
        (">=", &constraint[2..])
    } else if constraint.starts_with("<=") {
        ("<=", &constraint[2..])
    } else if constraint.starts_with('^') {
        ("^", &constraint[1..])
    } else if constraint.starts_with('~') {
        ("~", &constraint[1..])
    } else if constraint.starts_with('>') {
        (">", &constraint[1..])
    } else if constraint.starts_with('<') {
        ("<", &constraint[1..])
    } else {
        ("=", constraint)
    };

    let target = target.trim();

    let parse = |s: &str| -> (u64, u64, u64) {
        let parts: Vec<u64> = s
            .split('-')
            .next()
            .unwrap_or("")
            .split('.')
            .filter_map(|p| p.parse().ok())
            .collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        )
    };

    let (v_major, v_minor, v_patch) = parse(version);
    let (t_major, t_minor, t_patch) = parse(target);

    match operator {
        "=" => v_major == t_major && v_minor == t_minor && v_patch == t_patch,
        ">" => compare_versions(version, target) == std::cmp::Ordering::Greater,
        ">=" => compare_versions(version, target) != std::cmp::Ordering::Less,
        "<" => compare_versions(version, target) == std::cmp::Ordering::Less,
        "<=" => compare_versions(version, target) != std::cmp::Ordering::Greater,
        "^" => {
            if t_major == 0 {
                v_major == 0 && v_minor == t_minor && v_patch >= t_patch
            } else {
                v_major == t_major && (v_minor > t_minor || (v_minor == t_minor && v_patch >= t_patch))
            }
        }
        "~" => v_major == t_major && v_minor == t_minor && v_patch >= t_patch,
        _ => true,
    }
}

/// Resolve dependency conflicts automatically
pub fn resolve_conflicts(args: &Value) -> Result<Value> {
    let apply = args
        .get("apply")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Change to path if specified
    let path = args.get("path").and_then(|v| v.as_str());
    let original_dir = if let Some(dir) = path {
        let orig = env::current_dir()?;
        env::set_current_dir(dir)?;
        Some(orig)
    } else {
        None
    };

    // Run npm install --dry-run to detect conflicts
    let output = std::process::Command::new("cmd")
        .args(["/C", "npm install --dry-run 2>&1"])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            if let Some(orig) = original_dir {
                let _ = env::set_current_dir(orig);
            }
            anyhow::bail!("Failed to run npm: {}", e);
        }
    };

    let stderr = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);

    // Parse conflicts
    let conflicts = parse_conflicts(&stderr);

    if conflicts.is_empty() {
        if let Some(orig) = original_dir {
            let _ = env::set_current_dir(orig);
        }
        return Ok(serde_json::json!({
            "success": true,
            "conflicts": [],
            "resolutions": [],
            "message": "No dependency conflicts detected"
        }));
    }

    // Find resolutions
    let mut resolutions = Vec::new();
    for conflict in &conflicts {
        if let Some(res) = find_conflict_resolution(conflict) {
            resolutions.push(res);
        }
    }

    // Apply if requested
    let mut applied = Vec::new();
    if apply && !resolutions.is_empty() {
        if let Ok(content) = std::fs::read_to_string("package.json") {
            if let Ok(mut pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                for res in &resolutions {
                    let new_version = format!("^{}", res.get("suggested_version").and_then(|v| v.as_str()).unwrap_or(""));
                    let pkg_name = res.get("package").and_then(|v| v.as_str()).unwrap_or("");

                    if let Some(deps) = pkg.get_mut("dependencies").and_then(|d| d.as_object_mut()) {
                        if deps.contains_key(pkg_name) {
                            deps.insert(pkg_name.to_string(), serde_json::Value::String(new_version.clone()));
                            applied.push(pkg_name.to_string());
                        }
                    }
                    if let Some(deps) = pkg.get_mut("devDependencies").and_then(|d| d.as_object_mut()) {
                        if deps.contains_key(pkg_name) {
                            deps.insert(pkg_name.to_string(), serde_json::Value::String(new_version));
                            applied.push(pkg_name.to_string());
                        }
                    }
                }

                if !applied.is_empty() {
                    let _ = std::fs::write("package.json", serde_json::to_string_pretty(&pkg).unwrap_or_default());
                }
            }
        }
    }

    if let Some(orig) = original_dir {
        let _ = env::set_current_dir(orig);
    }

    Ok(serde_json::json!({
        "success": true,
        "conflicts": conflicts,
        "resolutions": resolutions,
        "applied": applied,
        "message": if apply && !applied.is_empty() {
            format!("Applied {} resolution(s). Run 'npm install' to complete.", applied.len())
        } else if resolutions.is_empty() {
            "Found conflicts but no automatic resolutions available".to_string()
        } else {
            format!("Found {} resolution(s). Set apply=true to update package.json", resolutions.len())
        }
    }))
}

/// Parse npm error output for conflicts
fn parse_conflicts(output: &str) -> Vec<serde_json::Value> {
    let mut conflicts = Vec::new();
    let mut current_package = String::new();
    let mut conflicting_dep = String::new();
    let mut required_range = String::new();
    let mut actual_version = String::new();
    let mut suggested_version = String::new();
    let mut found_eresolve = false;
    let mut found_dep_from_found_line = String::new();

    for line in output.lines() {
        let line = line.trim();

        // Track if we're in an ERESOLVE block
        if line.contains("ERESOLVE") {
            found_eresolve = true;
        }

        // "While resolving: react-native@0.81.5"
        if line.contains("While resolving:") {
            if let Some(pkg) = line.split("While resolving:").nth(1) {
                let pkg = pkg.trim();
                if let Some((name, _ver)) = pkg.rsplit_once('@') {
                    current_package = name.to_string();
                }
            }
        }

        // "Found: @types/react@19.0.14" - what we HAVE installed
        if line.contains("Found:") && !line.contains("node_modules") {
            if let Some(pkg) = line.split("Found:").nth(1) {
                let pkg = pkg.trim();
                if let Some((name, ver)) = pkg.rsplit_once('@') {
                    conflicting_dep = name.to_string();
                    actual_version = ver.to_string();
                    found_dep_from_found_line = name.to_string();
                }
            }
        }

        // "peerOptional @types/react@"^19.1.0" from react-native@0.81.5"
        // Only update if dep matches what we found in "Found:" line
        if (line.contains("peer ") || line.contains("peerOptional ")) && line.contains(" from ") {
            let peer_start = if let Some(pos) = line.find("peerOptional ") {
                pos + 13
            } else if let Some(pos) = line.find("peer ") {
                pos + 5
            } else {
                continue;
            };

            let after_peer = &line[peer_start..];
            if let Some(from_idx) = after_peer.find(" from ") {
                let requirement = after_peer[..from_idx].trim();
                if let Some((dep, range)) = requirement.rsplit_once('@') {
                    let range = range.trim_matches('"').trim_matches('\'');
                    // Only update if this matches the dep from "Found:" line
                    if !dep.is_empty() && (dep == found_dep_from_found_line || (required_range.is_empty() && conflicting_dep == dep)) {
                        conflicting_dep = dep.to_string();
                        required_range = range.to_string();
                    }
                }
            }
        }

        // "Conflicting peer dependency: @types/react@19.2.8" - npm's suggested version
        if line.contains("Conflicting peer dependency:") {
            if let Some(pkg) = line.split("Conflicting peer dependency:").nth(1) {
                let pkg = pkg.trim();
                if let Some((name, ver)) = pkg.rsplit_once('@') {
                    if name == conflicting_dep || name == found_dep_from_found_line {
                        conflicting_dep = name.to_string();
                        suggested_version = ver.to_string();
                    }
                }
            }
        }

        // "Could not resolve dependency:" signals end of conflict block
        if line.contains("Could not resolve dependency") {
            if !conflicting_dep.is_empty() && !actual_version.is_empty() {
                conflicts.push(serde_json::json!({
                    "package": conflicting_dep.clone(),
                    "current_version": actual_version.clone(),
                    "conflicting_dep": current_package.clone(),
                    "required_range": required_range.clone(),
                    "actual_version": actual_version.clone(),
                    "suggested_version": if !suggested_version.is_empty() { Some(suggested_version.clone()) } else { None::<String> }
                }));
                suggested_version.clear();
            }
        }
    }

    // Capture final conflict if we found ERESOLVE but didn't hit "Could not resolve"
    if found_eresolve && !conflicting_dep.is_empty() && !actual_version.is_empty()
       && conflicts.iter().all(|c| c.get("package").and_then(|p| p.as_str()) != Some(&conflicting_dep)) {
        conflicts.push(serde_json::json!({
            "package": conflicting_dep,
            "current_version": actual_version.clone(),
            "conflicting_dep": current_package,
            "required_range": required_range,
            "actual_version": actual_version,
            "suggested_version": if !suggested_version.is_empty() { Some(suggested_version) } else { None::<String> }
        }));
    }

    conflicts
}

/// Find resolution for a conflict
fn find_conflict_resolution(conflict: &serde_json::Value) -> Option<serde_json::Value> {
    let package = conflict.get("package")?.as_str()?;
    let conflicting_dep = conflict.get("conflicting_dep")?.as_str()?;
    let required_range = conflict.get("required_range").and_then(|r| r.as_str()).unwrap_or("");
    let actual_version = conflict.get("actual_version")?.as_str()?;

    // Fetch package info for the package that needs updating
    let encoded = package.replace("/", "%2f");
    let url = format!("https://registry.npmjs.org/{}", encoded);

    let response = reqwest::blocking::Client::new()
        .get(&url)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let info: serde_json::Value = response.json().ok()?;
    let versions = info.get("versions")?.as_object()?;

    // Sort versions (newest first)
    let mut version_list: Vec<&String> = versions.keys().collect();
    version_list.sort_by(|a, b| compare_versions(b, a));

    // Case 1: Direct dependency update - find version satisfying required_range
    if !required_range.is_empty() {
        for version_str in &version_list {
            if version_str.contains('-') {
                continue; // Skip pre-release
            }
            if matches_version_constraint(version_str, required_range) {
                return Some(serde_json::json!({
                    "package": package,
                    "current_version": actual_version,
                    "suggested_version": version_str,
                    "reason": format!("{} requires {} {}", conflicting_dep, package, required_range)
                }));
            }
        }
    }

    // Case 2: Library update needed - find version whose peer dep accepts installed version
    for version_str in version_list {
        if version_str.contains('-') {
            continue; // Skip pre-release
        }

        if let Some(ver_info) = versions.get(version_str) {
            if let Some(peers) = ver_info.get("peerDependencies").and_then(|p| p.as_object()) {
                if let Some(req) = peers.get(conflicting_dep) {
                    let req_str = req.as_str().unwrap_or("");
                    if matches_version_constraint(actual_version, req_str) {
                        return Some(serde_json::json!({
                            "package": package,
                            "current_version": actual_version,
                            "suggested_version": version_str,
                            "reason": format!("v{} supports {} (requires {})", version_str, conflicting_dep, req_str)
                        }));
                    }
                }
            }
        }
    }

    None
}
