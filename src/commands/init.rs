use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use crate::lockfile::EnvLock;
use crate::output::{OutputFormat, ZenvoOutput};

pub fn run(force: bool, format: OutputFormat) -> Result<()> {
    let lockfile_path = Path::new("env.lock");

    if lockfile_path.exists() && !force {
        if format == OutputFormat::Json {
            let output = ZenvoOutput::new("init")
                .with_success(false)
                .with_data(serde_json::json!({
                    "message": "env.lock already exists",
                    "hint": "Use --force to overwrite"
                }));
            println!("{}", output.to_json()?);
        } else {
            println!(
                "{} env.lock already exists. Use {} to overwrite.",
                "Warning:".yellow().bold(),
                "--force".cyan()
            );
        }
        return Ok(());
    }

    if format == OutputFormat::Text {
        println!("{}", "Initializing Zenvo...".cyan());
    }

    // Generate initial env.lock
    let env_lock = EnvLock::generate()?;
    env_lock.save(lockfile_path)?;

    if format == OutputFormat::Json {
        let output = ZenvoOutput::new("init")
            .with_success(true)
            .with_data(serde_json::json!({
                "created": true,
                "path": "env.lock",
                "toolchain": {
                    "node": env_lock.toolchain.node,
                    "package_manager": env_lock.toolchain.package_manager,
                    "package_manager_version": env_lock.toolchain.package_manager_version
                }
            }));
        println!("{}", output.to_json()?);
    } else {
        println!("{} Created env.lock", "✓".green().bold());
        println!();
        println!("Next steps:");
        println!("  1. Commit {} to your repository", "env.lock".cyan());
        println!("  2. Run {} to check your environment", "zenvo doctor".cyan());
        println!("  3. Run {} before each commit", "zenvo verify".cyan());
    }

    Ok(())
}
