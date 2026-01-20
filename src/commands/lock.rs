use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use crate::lockfile::EnvLock;
use crate::output::{OutputFormat, ZenvoOutput};

pub fn run(full: bool, format: OutputFormat) -> Result<()> {
    if format == OutputFormat::Text {
        println!("{}", "Generating env.lock...".cyan());
    }

    let mut env_lock = EnvLock::generate()?;

    if full {
        env_lock.include_system_info()?;
    }

    let lockfile_path = Path::new("env.lock");
    env_lock.save(lockfile_path)?;

    if format == OutputFormat::Json {
        let mut data = serde_json::json!({
            "updated": true,
            "path": "env.lock",
            "toolchain": {
                "node": env_lock.toolchain.node,
                "package_manager": env_lock.toolchain.package_manager,
                "package_manager_version": env_lock.toolchain.package_manager_version
            }
        });

        if let Some(ref lockfile) = env_lock.lockfile {
            data["lockfile"] = serde_json::json!({
                "type": lockfile.lockfile_type,
                "hash": lockfile.hash
            });
        }

        if let Some(ref environment) = env_lock.environment {
            data["environment"] = serde_json::json!({
                "os": environment.os,
                "arch": environment.arch
            });
        }

        let output = ZenvoOutput::new("lock")
            .with_success(true)
            .with_data(data);
        println!("{}", output.to_json()?);
    } else {
        println!("{} env.lock updated", "✓".green().bold());
        println!();
        println!("{}", "Locked environment:".bold());
        println!("  Node.js:         {}", env_lock.toolchain.node.cyan());
        println!(
            "  Package Manager: {} {}",
            env_lock.toolchain.package_manager.cyan(),
            env_lock.toolchain.package_manager_version.dimmed()
        );

        if let Some(ref lockfile) = env_lock.lockfile {
            println!("  Lockfile:        {}", lockfile.lockfile_type.cyan());
        }
    }

    Ok(())
}
