use anyhow::Result;
use colored::Colorize;

use crate::checks::detect_current_environment;
use crate::lockfile::EnvLock;
use crate::output::{EnvironmentStatus, OutputFormat, ZenvoOutput};

pub fn run(format: OutputFormat) -> Result<()> {
    // Detect current environment
    let current = detect_current_environment()?;

    // Try to load env.lock
    let locked = EnvLock::load_if_exists()?;

    // Check for drift
    let has_drift = if let Some(ref lock) = locked {
        current.node_version != lock.toolchain.node
            || current.package_manager != lock.toolchain.package_manager
            || current.package_manager_version != lock.toolchain.package_manager_version
    } else {
        false
    };

    if format == OutputFormat::Json {
        let env_status = EnvironmentStatus::from(&current).with_env_lock(locked.is_some());

        let mut data = serde_json::json!({
            "current": {
                "node": current.node_version,
                "package_manager": current.package_manager,
                "package_manager_version": current.package_manager_version,
                "lockfile_type": current.lockfile_type,
                "lockfile_hash": current.lockfile_hash
            },
            "has_env_lock": locked.is_some()
        });

        if let Some(ref lock) = locked {
            data["locked"] = serde_json::json!({
                "node": lock.toolchain.node,
                "package_manager": lock.toolchain.package_manager,
                "package_manager_version": lock.toolchain.package_manager_version
            });
        }

        let output = ZenvoOutput::new("status")
            .with_success(true)
            .with_drift(has_drift)
            .with_environment(env_status)
            .with_data(data);

        println!("{}", output.to_json()?);
    } else {
        println!("{}", "Environment Status".bold().cyan());
        println!("{}", "═".repeat(50).dimmed());
        println!();

        // Node.js
        println!("{}", "Node.js".bold());
        println!("  Current: {}", current.node_version.cyan());
        if let Some(ref lock) = locked {
            let matches = current.node_version == lock.toolchain.node;
            let status = if matches { "✓".green() } else { "✗".red() };
            println!("  Locked:  {} {}", lock.toolchain.node, status);
        } else {
            println!("  Locked:  {}", "(no env.lock)".dimmed());
        }
        println!();

        // Package Manager
        println!("{}", "Package Manager".bold());
        println!(
            "  Current: {} {}",
            current.package_manager.cyan(),
            current.package_manager_version.dimmed()
        );
        if let Some(ref lock) = locked {
            let matches = current.package_manager == lock.toolchain.package_manager
                && current.package_manager_version == lock.toolchain.package_manager_version;
            let status = if matches { "✓".green() } else { "✗".red() };
            println!(
                "  Locked:  {} {} {}",
                lock.toolchain.package_manager,
                lock.toolchain.package_manager_version.dimmed(),
                status
            );
        }
        println!();

        // Lockfile
        println!("{}", "Lockfile".bold());
        if let Some(ref lockfile_type) = current.lockfile_type {
            println!("  Type: {}", lockfile_type.cyan());
            println!(
                "  Hash: {}",
                current.lockfile_hash.as_deref().unwrap_or("N/A").dimmed()
            );
        } else {
            println!("  {}", "No lockfile found".yellow());
        }
        println!();

        // env.lock status
        if locked.is_some() {
            println!("{} env.lock found", "✓".green());
        } else {
            println!(
                "{} No env.lock - run {} to create one",
                "⚠".yellow(),
                "zenvo init".cyan()
            );
        }
    }

    Ok(())
}
