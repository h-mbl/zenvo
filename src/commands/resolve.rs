//! Resolve dependency conflicts automatically

use anyhow::{Context, Result};
use colored::Colorize;
use serde::Serialize;
use std::io::{self, Write};
use std::process::Command;

use crate::output::OutputFormat;

/// A detected dependency conflict
#[derive(Debug, Clone, Serialize)]
pub struct DependencyConflict {
    /// Package that has the conflict
    pub package: String,
    /// Current version in package.json
    pub current_version: String,
    /// The dependency causing the conflict
    pub conflicting_dep: String,
    /// What the conflicting dep requires
    pub required_range: String,
    /// What we actually have
    pub actual_version: String,
}

/// Suggested fix for a conflict
#[derive(Debug, Clone, Serialize)]
pub struct ConflictResolution {
    pub package: String,
    pub current_version: String,
    pub suggested_version: String,
    pub reason: String,
}

pub fn run(dry_run: bool, format: OutputFormat) -> Result<()> {
    if format != OutputFormat::Json {
        println!("Analyzing dependency conflicts...");
        println!();
    }

    // Step 1: Run npm install --dry-run to detect conflicts
    let conflicts = detect_conflicts()?;

    if conflicts.is_empty() {
        if format == OutputFormat::Json {
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "success": true,
                "conflicts": [],
                "message": "No dependency conflicts detected"
            }))?);
        } else {
            println!("{} No dependency conflicts detected!", "✓".green());
        }
        return Ok(());
    }

    if format != OutputFormat::Json {
        println!("{} Found {} conflict(s):", "⚠".yellow(), conflicts.len());
        println!();
        for conflict in &conflicts {
            println!("  {} {} @ {}", "•".red(), conflict.package.cyan(), conflict.current_version);
            println!("    Required by: {} (needs {})", conflict.conflicting_dep, conflict.required_range.green());
            println!("    Current version: {}", conflict.actual_version.red());
            println!();
        }
    }

    // Step 2: Search for compatible versions
    if format != OutputFormat::Json {
        println!("Searching for compatible versions...");
        println!();
    }

    let mut resolutions = Vec::new();
    for conflict in &conflicts {
        if let Some(resolution) = find_resolution(&conflict)? {
            resolutions.push(resolution);
        }
    }

    if resolutions.is_empty() {
        if format == OutputFormat::Json {
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "success": false,
                "conflicts": conflicts,
                "resolutions": [],
                "message": "Could not find automatic resolutions"
            }))?);
        } else {
            println!("{} Could not find automatic resolutions.", "✗".red());
            println!("  Try updating packages manually or use --legacy-peer-deps");
        }
        return Ok(());
    }

    // Step 3: Show suggested fixes
    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "success": true,
            "conflicts": conflicts,
            "resolutions": resolutions,
            "dry_run": dry_run
        }))?);
        return Ok(());
    }

    println!("{}", "Suggested Resolutions:".bold());
    println!();
    for res in &resolutions {
        println!("  {} {} → {}",
            "→".green(),
            res.package.cyan(),
            res.suggested_version.green()
        );
        println!("    {}", res.reason.dimmed());
        println!();
    }

    if dry_run {
        println!("{}", "Dry run - no changes made.".dimmed());
        println!("Run {} to apply changes.", "zenvo resolve".cyan());
        return Ok(());
    }

    // Step 4: Ask for confirmation
    print!("Apply these changes? [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim().to_lowercase() != "y" {
        println!("Cancelled.");
        return Ok(());
    }

    // Step 5: Apply changes
    apply_resolutions(&resolutions)?;

    println!();
    println!("{} Changes applied. Run {} to install.", "✓".green(), "npm install".cyan());

    Ok(())
}

/// Detect conflicts by running npm install --dry-run
fn detect_conflicts() -> Result<Vec<DependencyConflict>> {
    let output = Command::new("cmd")
        .args(["/C", "npm install --dry-run 2>&1"])
        .output()
        .context("Failed to run npm install --dry-run")?;

    let stderr = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);

    parse_npm_conflicts(&stderr)
}

/// Parse npm error output to extract conflicts
fn parse_npm_conflicts(output: &str) -> Result<Vec<DependencyConflict>> {
    let mut conflicts = Vec::new();
    let mut current_package = String::new();
    let mut conflicting_dep = String::new();
    let mut required_range = String::new();
    let mut actual_version = String::new();
    let mut suggested_version = String::new();
    let mut found_eresolve = false;
    let mut found_dep_from_found_line = String::new(); // The dep name from "Found:" line

    for line in output.lines() {
        let line = line.trim();

        // Track if we're in an ERESOLVE block
        if line.contains("ERESOLVE") {
            found_eresolve = true;
        }

        // "While resolving: react-native@0.81.5" or "@shopify/react-native-skia@1.12.4"
        if line.contains("While resolving:") {
            if let Some(pkg) = line.split("While resolving:").nth(1) {
                let pkg = pkg.trim();
                // Use rsplit_once to find the LAST @ (version separator, not scope prefix)
                if let Some((name, _ver)) = pkg.rsplit_once('@') {
                    current_package = name.to_string();
                }
            }
        }

        // "Found: @types/react@19.0.14" - this is what we HAVE installed
        if line.contains("Found:") && !line.contains("node_modules") {
            if let Some(pkg) = line.split("Found:").nth(1) {
                let pkg = pkg.trim();
                // Use rsplit_once to handle scoped packages like @types/react
                if let Some((name, ver)) = pkg.rsplit_once('@') {
                    conflicting_dep = name.to_string();
                    actual_version = ver.to_string();
                    found_dep_from_found_line = name.to_string();
                }
            }
        }

        // "peerOptional @types/react@"^19.1.0" from react-native@0.81.5"
        // "peer react@">=18.0 <19.0.0" from @shopify/react-native-skia@1.12.4"
        // This is what the package REQUIRES
        // Only update if the dep matches what we found in "Found:" line
        if (line.contains("peer ") || line.contains("peerOptional ")) && line.contains(" from ") {
            // Find where the peer requirement starts
            let peer_start = if let Some(pos) = line.find("peerOptional ") {
                pos + 13 // "peerOptional " length
            } else if let Some(pos) = line.find("peer ") {
                pos + 5 // "peer " length
            } else {
                continue;
            };

            let after_peer = &line[peer_start..];
            if let Some(from_idx) = after_peer.find(" from ") {
                let requirement = after_peer[..from_idx].trim();
                // Use rsplit_once to find the LAST @ (version separator)
                if let Some((dep, range)) = requirement.rsplit_once('@') {
                    let range = range.trim_matches('"').trim_matches('\'');
                    // Only update if this matches the dep from "Found:" line
                    // or if we haven't captured a range yet for this dep
                    if !dep.is_empty() && (dep == found_dep_from_found_line || (required_range.is_empty() && conflicting_dep == dep)) {
                        conflicting_dep = dep.to_string();
                        required_range = range.to_string();
                    }
                }
            }
        }

        // "Conflicting peer dependency: @types/react@19.2.8" - npm suggests this version
        if line.contains("Conflicting peer dependency:") {
            if let Some(pkg) = line.split("Conflicting peer dependency:").nth(1) {
                let pkg = pkg.trim();
                if let Some((name, ver)) = pkg.rsplit_once('@') {
                    // This is the version npm suggests we upgrade to
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
                conflicts.push(DependencyConflict {
                    package: conflicting_dep.clone(),
                    current_version: actual_version.clone(),
                    conflicting_dep: current_package.clone(),
                    required_range: required_range.clone(),
                    actual_version: if !suggested_version.is_empty() {
                        format!("{} (suggested: {})", actual_version.clone(), suggested_version.clone())
                    } else {
                        actual_version.clone()
                    },
                });
                // Reset for next conflict
                suggested_version.clear();
            }
        }
    }

    // Capture final conflict if we found ERESOLVE but didn't hit "Could not resolve"
    if found_eresolve && !conflicting_dep.is_empty() && !actual_version.is_empty()
       && conflicts.iter().all(|c| c.package != conflicting_dep) {
        conflicts.push(DependencyConflict {
            package: conflicting_dep,
            current_version: actual_version.clone(),
            conflicting_dep: current_package,
            required_range,
            actual_version: if !suggested_version.is_empty() {
                format!("{} (suggested: {})", actual_version, suggested_version)
            } else {
                actual_version
            },
        });
    }

    Ok(conflicts)
}

/// Find a resolution for a conflict by searching npm registry
fn find_resolution(conflict: &DependencyConflict) -> Result<Option<ConflictResolution>> {
    // Search for versions of the package that needs updating
    let encoded = conflict.package.replace("/", "%2f");
    let url = format!("https://registry.npmjs.org/{}", encoded);

    let response = reqwest::blocking::Client::new()
        .get(&url)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send();

    let response = match response {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };

    if !response.status().is_success() {
        return Ok(None);
    }

    let info: serde_json::Value = response.json()?;

    // Get available versions
    let versions = match info.get("versions").and_then(|v| v.as_object()) {
        Some(v) => v,
        None => return Ok(None),
    };

    // Get all version strings and sort them (newest first)
    let mut version_list: Vec<&String> = versions.keys().collect();
    version_list.sort_by(|a, b| compare_versions(b, a));

    // Case 1: Direct dependency update (e.g., @types/react needs to satisfy ^19.1.0)
    // If required_range is specified, find a version of the package that satisfies it
    if !conflict.required_range.is_empty() {
        for version_str in &version_list {
            // Skip pre-release versions unless current is also pre-release
            if version_str.contains('-') && !conflict.current_version.contains('-') {
                continue;
            }

            // Check if this version satisfies the required range
            if version_satisfies(version_str, &conflict.required_range) {
                return Ok(Some(ConflictResolution {
                    package: conflict.package.clone(),
                    current_version: conflict.current_version.clone(),
                    suggested_version: version_str.to_string(),
                    reason: format!(
                        "{} requires {} {}",
                        conflict.conflicting_dep,
                        conflict.package,
                        conflict.required_range
                    ),
                }));
            }
        }
    }

    // Case 2: Library update needed (e.g., @shopify/react-native-skia needs newer version)
    // Find a version of the package whose peer dependency accepts the installed version
    for version_str in &version_list {
        // Skip pre-release versions unless current is also pre-release
        if version_str.contains('-') && !conflict.current_version.contains('-') {
            continue;
        }

        if let Some(ver_info) = versions.get(*version_str) {
            let peer_deps = ver_info
                .get("peerDependencies")
                .and_then(|p| p.as_object());

            if let Some(peers) = peer_deps {
                // Check if this version's peer dep requirement includes our actual version
                if let Some(req) = peers.get(&conflict.conflicting_dep) {
                    let req_str = req.as_str().unwrap_or("");
                    // Extract actual version from "19.0.14 (suggested: 19.2.8)" format
                    let actual = conflict.actual_version.split(" (").next().unwrap_or(&conflict.actual_version);

                    if version_satisfies(actual, req_str) {
                        return Ok(Some(ConflictResolution {
                            package: conflict.package.clone(),
                            current_version: conflict.current_version.clone(),
                            suggested_version: version_str.to_string(),
                            reason: format!(
                                "v{} supports {} (requires {})",
                                version_str,
                                conflict.conflicting_dep,
                                req_str
                            ),
                        }));
                    }
                } else {
                    // No peer dep requirement for this dependency = compatible
                    return Ok(Some(ConflictResolution {
                        package: conflict.package.clone(),
                        current_version: conflict.current_version.clone(),
                        suggested_version: version_str.to_string(),
                        reason: format!(
                            "v{} has no peer requirement for {}",
                            version_str,
                            conflict.conflicting_dep
                        ),
                    }));
                }
            }
        }
    }

    // Could not find a resolution
    Ok(None)
}

/// Compare two version strings for sorting (returns ordering for descending sort)
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

/// Check if a version satisfies a semver range (simplified)
fn version_satisfies(version: &str, range: &str) -> bool {
    let range = range.trim();

    // Handle common patterns
    if range == "*" || range.is_empty() {
        return true;
    }

    // Parse version
    let parse_ver = |s: &str| -> (u32, u32, u32) {
        let parts: Vec<u32> = s
            .trim_start_matches('v')
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

    let (v_major, v_minor, v_patch) = parse_ver(version);

    // Handle >=X <Y patterns
    if range.contains(" ") {
        let parts: Vec<&str> = range.split_whitespace().collect();
        return parts.iter().all(|part| version_satisfies(version, part));
    }

    // Handle ^X.Y.Z
    if let Some(target) = range.strip_prefix('^') {
        let (t_major, t_minor, t_patch) = parse_ver(target);
        if t_major == 0 {
            return v_major == 0 && v_minor == t_minor && v_patch >= t_patch;
        }
        return v_major == t_major && (v_minor > t_minor || (v_minor == t_minor && v_patch >= t_patch));
    }

    // Handle ~X.Y.Z
    if let Some(target) = range.strip_prefix('~') {
        let (t_major, t_minor, t_patch) = parse_ver(target);
        return v_major == t_major && v_minor == t_minor && v_patch >= t_patch;
    }

    // Handle >=X.Y.Z
    if let Some(target) = range.strip_prefix(">=") {
        let (t_major, t_minor, t_patch) = parse_ver(target);
        return v_major > t_major
            || (v_major == t_major && v_minor > t_minor)
            || (v_major == t_major && v_minor == t_minor && v_patch >= t_patch);
    }

    // Handle <X.Y.Z
    if let Some(target) = range.strip_prefix('<') {
        let target = target.trim_start_matches('=');
        let (t_major, t_minor, t_patch) = parse_ver(target);
        return v_major < t_major
            || (v_major == t_major && v_minor < t_minor)
            || (v_major == t_major && v_minor == t_minor && v_patch < t_patch);
    }

    // Exact match
    let (t_major, t_minor, t_patch) = parse_ver(range);
    v_major == t_major && v_minor == t_minor && v_patch == t_patch
}

/// Apply resolutions by updating package.json
fn apply_resolutions(resolutions: &[ConflictResolution]) -> Result<()> {
    let pkg_path = "package.json";
    let content = std::fs::read_to_string(pkg_path)
        .context("Failed to read package.json")?;

    let mut pkg: serde_json::Value = serde_json::from_str(&content)
        .context("Failed to parse package.json")?;

    for res in resolutions {
        // Check dependencies
        if let Some(deps) = pkg.get_mut("dependencies").and_then(|d| d.as_object_mut()) {
            if deps.contains_key(&res.package) {
                deps.insert(
                    res.package.clone(),
                    serde_json::Value::String(format!("^{}", res.suggested_version)),
                );
                println!("  {} Updated {} in dependencies", "✓".green(), res.package);
            }
        }

        // Check devDependencies
        if let Some(deps) = pkg.get_mut("devDependencies").and_then(|d| d.as_object_mut()) {
            if deps.contains_key(&res.package) {
                deps.insert(
                    res.package.clone(),
                    serde_json::Value::String(format!("^{}", res.suggested_version)),
                );
                println!("  {} Updated {} in devDependencies", "✓".green(), res.package);
            }
        }
    }

    // Write back
    let updated = serde_json::to_string_pretty(&pkg)?;
    std::fs::write(pkg_path, updated)
        .context("Failed to write package.json")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_satisfies() {
        assert!(version_satisfies("19.1.0", ">=18.0.0"));
        assert!(version_satisfies("18.3.1", ">=18.0 <19.0.0"));
        assert!(!version_satisfies("19.1.0", ">=18.0 <19.0.0"));
        assert!(version_satisfies("18.2.0", "^18.0.0"));
        assert!(!version_satisfies("19.0.0", "^18.0.0"));
        assert!(version_satisfies("1.2.5", "~1.2.3"));
        assert!(!version_satisfies("1.3.0", "~1.2.3"));
    }

    #[test]
    fn test_parse_npm_conflicts() {
        let output = r#"
npm error ERESOLVE could not resolve
npm error While resolving: @shopify/react-native-skia@1.12.4
npm error Found: react@19.1.0
npm error peer react@">=18.0 <19.0.0" from @shopify/react-native-skia@1.12.4
npm error Could not resolve dependency:
        "#;

        let conflicts = parse_npm_conflicts(output).unwrap();
        assert_eq!(conflicts.len(), 1);
        // The package that needs updating is "react" (the conflicting dep)
        assert_eq!(conflicts[0].package, "react");
        assert_eq!(conflicts[0].current_version, "19.1.0");
        assert_eq!(conflicts[0].required_range, ">=18.0 <19.0.0");
    }

    #[test]
    fn test_parse_npm_conflicts_scoped_package() {
        let output = r#"
npm error ERESOLVE could not resolve
npm error While resolving: react-native@0.81.5
npm error Found: @types/react@19.0.14
npm error peerOptional @types/react@"^19.1.0" from react-native@0.81.5
npm error Conflicting peer dependency: @types/react@19.2.8
npm error Could not resolve dependency:
        "#;

        let conflicts = parse_npm_conflicts(output).unwrap();
        assert_eq!(conflicts.len(), 1);
        // The package that needs updating is "@types/react"
        assert_eq!(conflicts[0].package, "@types/react");
        assert_eq!(conflicts[0].current_version, "19.0.14");
        assert_eq!(conflicts[0].required_range, "^19.1.0");
        // The conflicting package is react-native
        assert_eq!(conflicts[0].conflicting_dep, "react-native");
    }
}
