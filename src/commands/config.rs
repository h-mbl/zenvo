//! Configuration commands for Zenvo
//! Provides `zenvo config init` and `zenvo config validate` subcommands.

use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use crate::config::{ZenvoConfig, CONFIG_FILE};
use crate::output::{OutputFormat, ZenvoOutput};

/// Config subcommand action
#[derive(Debug, Clone)]
pub enum ConfigAction {
    Init { force: bool },
    Validate,
}

/// Run the config command
pub fn run(action: ConfigAction, format: OutputFormat) -> Result<()> {
    match action {
        ConfigAction::Init { force } => run_init(force, format),
        ConfigAction::Validate => run_validate(format),
    }
}

/// Create default .env.doctor.toml configuration file
fn run_init(force: bool, format: OutputFormat) -> Result<()> {
    let path = Path::new(CONFIG_FILE);

    // Check if file already exists
    if path.exists() && !force {
        if format == OutputFormat::Json {
            let output = ZenvoOutput::new("config init")
                .with_success(false)
                .with_data(serde_json::json!({
                    "error": "Config file already exists",
                    "path": CONFIG_FILE,
                    "hint": "Use --force to overwrite"
                }));
            println!("{}", output.to_json()?);
        } else {
            println!(
                "{} {} already exists",
                "Error:".red().bold(),
                CONFIG_FILE.cyan()
            );
            println!("Use {} to overwrite.", "--force".cyan());
        }
        return Ok(());
    }

    // Create default config
    let config = ZenvoConfig::create_default(path)?;

    if format == OutputFormat::Json {
        let output = ZenvoOutput::new("config init")
            .with_success(true)
            .with_data(serde_json::json!({
                "path": CONFIG_FILE,
                "created": true,
                "config": {
                    "policies": {
                        "allow_node_upgrade_minor": config.policies.allow_node_upgrade_minor,
                        "allow_node_upgrade_major": config.policies.allow_node_upgrade_major,
                        "require_lockfile_frozen": config.policies.require_lockfile_frozen,
                        "enforce_corepack": config.policies.enforce_corepack
                    }
                }
            }));
        println!("{}", output.to_json()?);
    } else {
        println!("{} Created {}", "✓".green().bold(), CONFIG_FILE.cyan());
        println!();
        println!("You can customize:");
        println!("  • {} - Control version upgrade policies", "[policies]".cyan());
        println!("  • {} - Disable specific checks", "[checks]".cyan());
        println!(
            "  • {} - Framework-specific settings",
            "[frameworks]".cyan()
        );
    }

    Ok(())
}

/// Validate the configuration file
fn run_validate(format: OutputFormat) -> Result<()> {
    let path = Path::new(CONFIG_FILE);

    // Check if config exists
    if !path.exists() {
        if format == OutputFormat::Json {
            let output = ZenvoOutput::new("config validate")
                .with_success(false)
                .with_data(serde_json::json!({
                    "error": "Config file not found",
                    "path": CONFIG_FILE,
                    "hint": "Run `zenvo config init` to create one"
                }));
            println!("{}", output.to_json()?);
        } else {
            println!(
                "{} {} not found",
                "Error:".red().bold(),
                CONFIG_FILE.cyan()
            );
            println!("Run {} to create one.", "zenvo config init".cyan());
        }
        return Ok(());
    }

    // Load and validate config
    match ZenvoConfig::load() {
        Ok(config) => {
            // Run additional validation
            match config.validate() {
                Ok(()) => {
                    if format == OutputFormat::Json {
                        let output = ZenvoOutput::new("config validate")
                            .with_success(true)
                            .with_data(serde_json::json!({
                                "path": CONFIG_FILE,
                                "valid": true,
                                "disabled_checks": config.checks.disabled.len(),
                                "severity_overrides": config.checks.severity_overrides.len()
                            }));
                        println!("{}", output.to_json()?);
                    } else {
                        println!("{} {} is valid", "✓".green().bold(), CONFIG_FILE.cyan());
                        println!();

                        // Show summary
                        if !config.checks.disabled.is_empty() {
                            println!(
                                "  {} disabled checks: {}",
                                config.checks.disabled.len(),
                                config.checks.disabled.join(", ").dimmed()
                            );
                        }

                        if !config.checks.severity_overrides.is_empty() {
                            println!(
                                "  {} severity overrides",
                                config.checks.severity_overrides.len()
                            );
                        }

                        if config.policies.enforce_corepack {
                            println!("  {} Corepack enforcement enabled", "→".cyan());
                        }

                        if let Some(ref min) = config.policies.min_node_version {
                            println!("  {} Minimum Node version: {}", "→".cyan(), min);
                        }
                    }
                }
                Err(e) => {
                    if format == OutputFormat::Json {
                        let output = ZenvoOutput::new("config validate")
                            .with_success(false)
                            .with_data(serde_json::json!({
                                "path": CONFIG_FILE,
                                "valid": false,
                                "error": e.to_string()
                            }));
                        println!("{}", output.to_json()?);
                    } else {
                        println!(
                            "{} {} has validation errors",
                            "✗".red().bold(),
                            CONFIG_FILE.cyan()
                        );
                        println!("  {}", e);
                    }
                }
            }
        }
        Err(e) => {
            if format == OutputFormat::Json {
                let output = ZenvoOutput::new("config validate")
                    .with_success(false)
                    .with_data(serde_json::json!({
                        "path": CONFIG_FILE,
                        "valid": false,
                        "error": e.to_string()
                    }));
                println!("{}", output.to_json()?);
            } else {
                println!(
                    "{} Failed to parse {}",
                    "✗".red().bold(),
                    CONFIG_FILE.cyan()
                );
                println!("  {}", e);
            }
        }
    }

    Ok(())
}
