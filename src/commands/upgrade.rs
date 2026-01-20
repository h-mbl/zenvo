use anyhow::Result;
use colored::Colorize;
use std::io::{self, Write};
use std::process::Command;

use crate::lockfile::EnvLock;
use crate::output::{OutputFormat, ZenvoOutput};

/// Upgrade result for a package
#[derive(Debug, Clone, serde::Serialize)]
pub struct PackageUpgrade {
    pub name: String,
    pub current: String,
    pub wanted: String,
    pub latest: String,
    pub upgrade_type: String,
}

pub fn run(interactive: bool, major: bool, dry_run: bool, format: OutputFormat) -> Result<()> {
    if format == OutputFormat::Text {
        println!("{}", "Checking for dependency updates...".cyan());
        println!();
    }

    // Get outdated packages
    let outdated = get_outdated_packages()?;

    if outdated.is_empty() {
        if format == OutputFormat::Json {
            let output = ZenvoOutput::new("upgrade")
                .with_success(true)
                .with_data(serde_json::json!({
                    "message": "All packages are up to date",
                    "packages": []
                }));
            println!("{}", output.to_json()?);
        } else {
            println!("{}", "All packages are up to date! ✨".green());
        }
        return Ok(());
    }

    // Categorize upgrades
    let mut patches: Vec<&PackageUpgrade> = Vec::new();
    let mut minors: Vec<&PackageUpgrade> = Vec::new();
    let mut majors: Vec<&PackageUpgrade> = Vec::new();

    for pkg in &outdated {
        match pkg.upgrade_type.as_str() {
            "patch" => patches.push(pkg),
            "minor" => minors.push(pkg),
            "major" => majors.push(pkg),
            _ => {}
        }
    }

    if format == OutputFormat::Json {
        let mut to_upgrade: Vec<&PackageUpgrade> = Vec::new();
        to_upgrade.extend(&patches);
        to_upgrade.extend(&minors);
        if major {
            to_upgrade.extend(&majors);
        }

        let output = ZenvoOutput::new("upgrade")
            .with_success(true)
            .with_data(serde_json::json!({
                "dry_run": dry_run,
                "include_major": major,
                "summary": {
                    "patch": patches.len(),
                    "minor": minors.len(),
                    "major": majors.len(),
                    "total": outdated.len()
                },
                "packages": outdated,
                "to_upgrade": to_upgrade
            }));

        println!("{}", output.to_json()?);

        if dry_run {
            return Ok(());
        }
    } else {
        // Text output
        println!("{}", "Upgrade Plan".bold().cyan());
        println!("{}", "═".repeat(60).dimmed());
        println!();

        if !patches.is_empty() {
            println!(
                "{} {} patch updates",
                "→".green(),
                patches.len().to_string().bold()
            );
            for pkg in &patches {
                println!(
                    "  {} {} → {}",
                    pkg.name.cyan(),
                    pkg.current.dimmed(),
                    pkg.wanted.green()
                );
            }
            println!();
        }

        if !minors.is_empty() {
            println!(
                "{} {} minor updates",
                "→".yellow(),
                minors.len().to_string().bold()
            );
            for pkg in &minors {
                println!(
                    "  {} {} → {}",
                    pkg.name.cyan(),
                    pkg.current.dimmed(),
                    pkg.wanted.yellow()
                );
            }
            println!();
        }

        if !majors.is_empty() {
            println!(
                "{} {} major updates {}",
                "→".red(),
                majors.len().to_string().bold(),
                if major {
                    "(will upgrade)"
                } else {
                    "(skipped without --major)"
                }
                .dimmed()
            );
            for pkg in &majors {
                println!(
                    "  {} {} → {}",
                    pkg.name.cyan(),
                    pkg.current.dimmed(),
                    pkg.latest.red()
                );
            }
            println!();
        }

        if dry_run {
            println!("{}", "Dry run - no changes made.".dimmed());
            println!(
                "Run {} to apply updates.",
                "zenvo upgrade".cyan()
            );
            return Ok(());
        }
    }

    // Build list of packages to upgrade
    let mut packages_to_upgrade: Vec<String> = Vec::new();

    // Always include patch and minor
    for pkg in &patches {
        packages_to_upgrade.push(format!("{}@{}", pkg.name, pkg.wanted));
    }
    for pkg in &minors {
        packages_to_upgrade.push(format!("{}@{}", pkg.name, pkg.wanted));
    }

    // Only include major if flag is set
    if major {
        for pkg in &majors {
            packages_to_upgrade.push(format!("{}@{}", pkg.name, pkg.latest));
        }
    }

    if packages_to_upgrade.is_empty() {
        if format == OutputFormat::Text {
            println!("{}", "No packages to upgrade (use --major for major updates)".yellow());
        }
        return Ok(());
    }

    // Confirm if interactive
    if interactive && format == OutputFormat::Text {
        print!(
            "Upgrade {} packages? [y/N] ",
            packages_to_upgrade.len().to_string().bold()
        );
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("{}", "Cancelled.".yellow());
            return Ok(());
        }
    }

    // Detect package manager
    let pkg_manager = detect_package_manager();

    // Execute upgrade
    if format == OutputFormat::Text {
        println!();
        println!("{}", "Upgrading packages...".cyan());
    }

    let install_args: Vec<&str> = match pkg_manager.as_str() {
        "pnpm" => vec!["add"],
        "yarn" => vec!["add"],
        _ => vec!["install"],
    };

    let mut cmd = Command::new(&pkg_manager);
    cmd.args(&install_args);
    for pkg in &packages_to_upgrade {
        cmd.arg(pkg);
    }

    let output = cmd.output()?;

    if output.status.success() {
        // Regenerate env.lock
        let env_lock = EnvLock::generate()?;
        env_lock.save(std::path::Path::new("env.lock"))?;

        if format == OutputFormat::Json {
            let output = ZenvoOutput::new("upgrade")
                .with_success(true)
                .with_data(serde_json::json!({
                    "message": "Upgrade completed successfully",
                    "upgraded": packages_to_upgrade,
                    "env_lock_updated": true
                }));
            println!("{}", output.to_json()?);
        } else {
            println!();
            println!(
                "{} Upgraded {} packages",
                "✓".green().bold(),
                packages_to_upgrade.len()
            );
            println!("{} env.lock updated", "✓".green().bold());
            println!();
            println!(
                "Run {} to verify.",
                "zenvo doctor".cyan()
            );
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if format == OutputFormat::Json {
            let output = ZenvoOutput::new("upgrade")
                .with_success(false)
                .with_data(serde_json::json!({
                    "error": "Upgrade failed",
                    "details": stderr.to_string()
                }));
            println!("{}", output.to_json()?);
        } else {
            println!("{} Upgrade failed: {}", "✗".red().bold(), stderr);
        }
    }

    Ok(())
}

/// Get outdated packages using npm outdated
fn get_outdated_packages() -> Result<Vec<PackageUpgrade>> {
    let output = Command::new("npm")
        .args(["outdated", "--json"])
        .output()?;

    // npm outdated returns exit code 1 if there are outdated packages
    // so we check the output regardless of exit code
    let json_str = String::from_utf8_lossy(&output.stdout);

    if json_str.trim().is_empty() {
        return Ok(Vec::new());
    }

    let outdated: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(_) => return Ok(Vec::new()),
    };

    let mut packages = Vec::new();

    if let Some(obj) = outdated.as_object() {
        for (name, info) in obj {
            let current = info
                .get("current")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0")
                .to_string();
            let wanted = info
                .get("wanted")
                .and_then(|v| v.as_str())
                .unwrap_or(&current)
                .to_string();
            let latest = info
                .get("latest")
                .and_then(|v| v.as_str())
                .unwrap_or(&wanted)
                .to_string();

            // Determine upgrade type
            let upgrade_type = determine_upgrade_type(&current, &wanted, &latest);

            packages.push(PackageUpgrade {
                name: name.clone(),
                current,
                wanted,
                latest,
                upgrade_type,
            });
        }
    }

    // Sort by upgrade type
    packages.sort_by(|a, b| {
        let order = |t: &str| match t {
            "patch" => 0,
            "minor" => 1,
            "major" => 2,
            _ => 3,
        };
        order(&a.upgrade_type).cmp(&order(&b.upgrade_type))
    });

    Ok(packages)
}

/// Determine if upgrade is patch, minor, or major
fn determine_upgrade_type(current: &str, wanted: &str, latest: &str) -> String {
    let parse_version = |v: &str| -> (u32, u32, u32) {
        let parts: Vec<u32> = v
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        (
            *parts.first().unwrap_or(&0),
            *parts.get(1).unwrap_or(&0),
            *parts.get(2).unwrap_or(&0),
        )
    };

    let (c_major, c_minor, _) = parse_version(current);
    let (w_major, w_minor, _) = parse_version(wanted);
    let (l_major, _, _) = parse_version(latest);

    if l_major > c_major {
        "major".to_string()
    } else if w_major > c_major {
        "major".to_string()
    } else if w_minor > c_minor {
        "minor".to_string()
    } else {
        "patch".to_string()
    }
}

/// Detect the package manager in use
fn detect_package_manager() -> String {
    // Check for packageManager field in package.json
    if let Ok(pkg_json) = std::fs::read_to_string("package.json") {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&pkg_json) {
            if let Some(pm) = pkg.get("packageManager").and_then(|v| v.as_str()) {
                if let Some((name, _)) = pm.split_once('@') {
                    return name.to_string();
                }
            }
        }
    }

    // Check for lockfile presence
    if std::path::Path::new("pnpm-lock.yaml").exists() {
        return "pnpm".to_string();
    }
    if std::path::Path::new("yarn.lock").exists() {
        return "yarn".to_string();
    }

    "npm".to_string()
}
