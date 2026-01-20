//! Search for available package versions on npm registry

use anyhow::{Context, Result};
use colored::Colorize;
use serde::Deserialize;
use std::collections::HashMap;

use crate::output::OutputFormat;

/// npm registry package metadata response
#[derive(Debug, Deserialize)]
struct NpmPackageInfo {
    #[allow(dead_code)]
    name: String,
    #[serde(rename = "dist-tags")]
    dist_tags: Option<HashMap<String, String>>,
    versions: Option<HashMap<String, serde_json::Value>>,
    time: Option<HashMap<String, String>>,
}

/// Version info for display
#[derive(Debug, Clone)]
struct VersionInfo {
    version: String,
    published: Option<String>,
    is_latest: bool,
    is_deprecated: bool,
}

pub fn run(
    package: &str,
    constraint: Option<&str>,
    limit: usize,
    show_all: bool,
    format: OutputFormat,
) -> Result<()> {
    if format != OutputFormat::Json {
        println!("Searching versions for {}...", package.cyan());
        println!();
    }

    // Fetch package info from npm registry
    let info = fetch_package_info(package)?;

    // Get versions sorted by semver (newest first)
    let mut versions = get_sorted_versions(&info)?;

    // Filter by constraint if provided
    if let Some(constraint_str) = constraint {
        versions = filter_by_constraint(versions, constraint_str);
    }

    // Limit results
    let display_versions: Vec<_> = if show_all {
        versions
    } else {
        versions.into_iter().take(limit).collect()
    };

    // Output
    if format == OutputFormat::Json {
        let json_output = serde_json::json!({
            "package": package,
            "constraint": constraint,
            "versions": display_versions.iter().map(|v| {
                serde_json::json!({
                    "version": v.version,
                    "published": v.published,
                    "latest": v.is_latest,
                    "deprecated": v.is_deprecated
                })
            }).collect::<Vec<_>>(),
            "total_available": info.versions.as_ref().map(|v| v.len()).unwrap_or(0)
        });
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        // Get dist-tags
        if let Some(dist_tags) = &info.dist_tags {
            println!("{}", "Distribution Tags:".bold());
            for (tag, version) in dist_tags {
                println!("  {} → {}", tag.yellow(), version.green());
            }
            println!();
        }

        println!("{}", "Available Versions:".bold());
        if display_versions.is_empty() {
            println!("  {}", "No versions found matching criteria".dimmed());
        } else {
            for v in &display_versions {
                let version_str = if v.is_latest {
                    format!("{} {}", v.version.green(), "(latest)".dimmed())
                } else if v.is_deprecated {
                    format!("{} {}", v.version.yellow(), "(deprecated)".red())
                } else {
                    v.version.clone()
                };

                let date_str = v
                    .published
                    .as_ref()
                    .map(|d| format!(" ({})", &d[..10]))
                    .unwrap_or_default();

                println!("  {} {}", version_str, date_str.dimmed());
            }
        }

        // Show suggestion for constraint
        if let Some(constraint_str) = constraint {
            println!();
            if display_versions.is_empty() {
                println!(
                    "{} No versions match constraint '{}'",
                    "⚠".yellow(),
                    constraint_str
                );
                println!("  Try a different constraint or use --all to see all versions");
            } else {
                println!(
                    "{} {} versions match constraint '{}'",
                    "✓".green(),
                    display_versions.len(),
                    constraint_str
                );
            }
        }

        // Show usage hint
        println!();
        println!("{}", "Usage:".dimmed());
        if let Some(latest) = display_versions.first() {
            println!(
                "  npm install {}@{}",
                package,
                latest.version
            );
        }
    }

    Ok(())
}

/// Fetch package info from npm registry
fn fetch_package_info(package: &str) -> Result<NpmPackageInfo> {
    // URL encode the package name (for scoped packages like @types/node)
    let encoded_package = package.replace("/", "%2f");
    let url = format!("https://registry.npmjs.org/{}", encoded_package);

    let response = reqwest::blocking::Client::new()
        .get(&url)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .context("Failed to connect to npm registry")?;

    if response.status() == 404 {
        anyhow::bail!("Package '{}' not found on npm registry", package);
    }

    if !response.status().is_success() {
        anyhow::bail!(
            "npm registry returned error: {} {}",
            response.status(),
            response.status().canonical_reason().unwrap_or("")
        );
    }

    let info: NpmPackageInfo = response
        .json()
        .context("Failed to parse npm registry response")?;

    Ok(info)
}

/// Get versions sorted by semver (newest first)
fn get_sorted_versions(info: &NpmPackageInfo) -> Result<Vec<VersionInfo>> {
    let versions_map = info
        .versions
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No versions found for package"))?;

    let latest = info
        .dist_tags
        .as_ref()
        .and_then(|tags| tags.get("latest"))
        .map(|s| s.as_str());

    let mut versions: Vec<VersionInfo> = versions_map
        .iter()
        .map(|(version, meta)| {
            let is_deprecated = meta
                .get("deprecated")
                .map(|d| !d.is_null())
                .unwrap_or(false);

            let published = info
                .time
                .as_ref()
                .and_then(|t| t.get(version))
                .cloned();

            VersionInfo {
                version: version.clone(),
                published,
                is_latest: latest == Some(version.as_str()),
                is_deprecated,
            }
        })
        .collect();

    // Sort by semver (newest first)
    versions.sort_by(|a, b| {
        compare_semver(&b.version, &a.version)
    });

    Ok(versions)
}

/// Simple semver comparison
fn compare_semver(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> (u64, u64, u64, String) {
        let clean = s.trim_start_matches('v');
        let parts: Vec<&str> = clean.split('-').collect();
        let version_parts: Vec<u64> = parts[0]
            .split('.')
            .filter_map(|p| p.parse().ok())
            .collect();

        let major = version_parts.first().copied().unwrap_or(0);
        let minor = version_parts.get(1).copied().unwrap_or(0);
        let patch = version_parts.get(2).copied().unwrap_or(0);
        let prerelease = parts.get(1).unwrap_or(&"").to_string();

        (major, minor, patch, prerelease)
    };

    let (a_major, a_minor, a_patch, a_pre) = parse(a);
    let (b_major, b_minor, b_patch, b_pre) = parse(b);

    match a_major.cmp(&b_major) {
        std::cmp::Ordering::Equal => match a_minor.cmp(&b_minor) {
            std::cmp::Ordering::Equal => match a_patch.cmp(&b_patch) {
                std::cmp::Ordering::Equal => {
                    // Prerelease versions come before release versions
                    match (a_pre.is_empty(), b_pre.is_empty()) {
                        (true, false) => std::cmp::Ordering::Greater,
                        (false, true) => std::cmp::Ordering::Less,
                        _ => a_pre.cmp(&b_pre),
                    }
                }
                other => other,
            },
            other => other,
        },
        other => other,
    }
}

/// Filter versions by semver constraint
fn filter_by_constraint(versions: Vec<VersionInfo>, constraint: &str) -> Vec<VersionInfo> {
    let constraint = constraint.trim();

    // Parse constraint
    let (operator, version_str) = if constraint.starts_with(">=") {
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
    } else if constraint.starts_with('=') {
        ("=", &constraint[1..])
    } else {
        ("=", constraint)
    };

    let version_str = version_str.trim();

    versions
        .into_iter()
        .filter(|v| matches_constraint(&v.version, operator, version_str))
        .collect()
}

/// Check if a version matches a constraint
fn matches_constraint(version: &str, operator: &str, constraint_version: &str) -> bool {
    let parse = |s: &str| -> (u64, u64, u64) {
        let clean = s.trim_start_matches('v');
        let parts: Vec<&str> = clean.split('-').collect();
        let version_parts: Vec<u64> = parts[0]
            .split('.')
            .filter_map(|p| p.parse().ok())
            .collect();

        let major = version_parts.first().copied().unwrap_or(0);
        let minor = version_parts.get(1).copied().unwrap_or(0);
        let patch = version_parts.get(2).copied().unwrap_or(0);

        (major, minor, patch)
    };

    let (v_major, v_minor, v_patch) = parse(version);
    let (c_major, c_minor, c_patch) = parse(constraint_version);

    match operator {
        "=" => v_major == c_major && v_minor == c_minor && v_patch == c_patch,
        ">" => {
            v_major > c_major
                || (v_major == c_major && v_minor > c_minor)
                || (v_major == c_major && v_minor == c_minor && v_patch > c_patch)
        }
        ">=" => {
            v_major > c_major
                || (v_major == c_major && v_minor > c_minor)
                || (v_major == c_major && v_minor == c_minor && v_patch >= c_patch)
        }
        "<" => {
            v_major < c_major
                || (v_major == c_major && v_minor < c_minor)
                || (v_major == c_major && v_minor == c_minor && v_patch < c_patch)
        }
        "<=" => {
            v_major < c_major
                || (v_major == c_major && v_minor < c_minor)
                || (v_major == c_major && v_minor == c_minor && v_patch <= c_patch)
        }
        "^" => {
            // Caret: allows changes that do not modify the left-most non-zero digit
            if c_major == 0 {
                if c_minor == 0 {
                    // ^0.0.x - only patch updates
                    v_major == 0 && v_minor == 0 && v_patch >= c_patch
                } else {
                    // ^0.x.y - minor and patch updates within 0.x
                    v_major == 0 && v_minor == c_minor && v_patch >= c_patch
                }
            } else {
                // ^x.y.z - minor and patch updates within major
                v_major == c_major && (v_minor > c_minor || (v_minor == c_minor && v_patch >= c_patch))
            }
        }
        "~" => {
            // Tilde: allows patch-level changes
            v_major == c_major && v_minor == c_minor && v_patch >= c_patch
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_semver() {
        assert_eq!(compare_semver("1.0.0", "1.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare_semver("2.0.0", "1.0.0"), std::cmp::Ordering::Greater);
        assert_eq!(compare_semver("1.0.0", "2.0.0"), std::cmp::Ordering::Less);
        assert_eq!(compare_semver("1.1.0", "1.0.0"), std::cmp::Ordering::Greater);
        assert_eq!(compare_semver("1.0.1", "1.0.0"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn test_matches_constraint_tilde() {
        assert!(matches_constraint("1.2.3", "~", "1.2.0"));
        assert!(matches_constraint("1.2.5", "~", "1.2.3"));
        assert!(!matches_constraint("1.3.0", "~", "1.2.3"));
        assert!(!matches_constraint("1.2.2", "~", "1.2.3"));
    }

    #[test]
    fn test_matches_constraint_caret() {
        assert!(matches_constraint("1.2.3", "^", "1.0.0"));
        assert!(matches_constraint("1.5.0", "^", "1.2.3"));
        assert!(!matches_constraint("2.0.0", "^", "1.2.3"));
        assert!(!matches_constraint("1.1.0", "^", "1.2.3"));
    }
}
