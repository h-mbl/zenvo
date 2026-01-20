use anyhow::Result;
use colored::Colorize;
use std::io::{self, Write};

use crate::checks::{run_all_checks, CheckSeverity};
use crate::config::ZenvoConfig;
use crate::lockfile::EnvLock;
use crate::output::{OutputFormat, RepairActionJson, RepairPlanOutput, ZenvoOutput};
use crate::repair::{execute_repair, generate_repair_plan_with_context, RepairContext};

pub fn run(plan: bool, apply: bool, auto_yes: bool, format: OutputFormat) -> Result<()> {
    if !plan && !apply {
        if format == OutputFormat::Json {
            let output = ZenvoOutput::new("repair")
                .with_success(false)
                .with_data(serde_json::json!({
                    "error": "Missing required flag",
                    "hint": "Use --plan or --apply"
                }));
            println!("{}", output.to_json()?);
        } else {
            println!(
                "{}",
                "Usage: zenvo repair --plan  OR  zenvo repair --apply".yellow()
            );
            println!();
            println!("  {} Show what would be fixed", "--plan".cyan());
            println!("  {} Execute the repair plan", "--apply".cyan());
            println!("  {} Auto-approve safe repairs", "-y".cyan());
        }
        return Ok(());
    }

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
        if format == OutputFormat::Json {
            let output = ZenvoOutput::new("repair")
                .with_success(true)
                .with_drift(false)
                .with_data(serde_json::json!({
                    "message": "No issues to repair",
                    "healthy": true
                }));
            println!("{}", output.to_json()?);
        } else {
            println!(
                "{}",
                "No issues to repair! Environment is healthy. ✨".green()
            );
        }
        return Ok(());
    }

    // Create repair context from env.lock
    let repair_context = RepairContext::new(&env_lock.toolchain.package_manager)
        .with_node_version_manager(env_lock.toolchain.node_version_source.clone())
        .with_target_node_version(Some(env_lock.toolchain.node.clone()));

    // Generate repair plan with context
    let repair_plan = generate_repair_plan_with_context(&issues, &repair_context)?;

    if plan {
        // Convert to JSON-friendly format
        let actions_json: Vec<RepairActionJson> = repair_plan
            .iter()
            .map(|a| RepairActionJson {
                description: a.description.clone(),
                command: a.command.clone(),
                is_safe: a.is_safe,
            })
            .collect();

        let safe_count = actions_json.iter().filter(|a| a.is_safe).count();
        let review_count = actions_json.len() - safe_count;

        if format == OutputFormat::Json {
            let plan_output = RepairPlanOutput {
                actions: actions_json,
                total_issues: issues.len(),
                safe_actions: safe_count,
                review_actions: review_count,
            };

            let output = ZenvoOutput::new("repair")
                .with_success(true)
                .with_drift(true)
                .with_data(serde_json::to_value(&plan_output)?);

            println!("{}", output.to_json()?);
        } else {
            // Show plan only
            println!("{}", "Repair Plan".bold().cyan());
            println!("{}", "═".repeat(50).dimmed());
            println!();

            for (i, action) in repair_plan.iter().enumerate() {
                let safety_badge = if action.is_safe {
                    "[SAFE]".green()
                } else {
                    "[REVIEW]".yellow()
                };

                println!(
                    "{}. {} {}",
                    (i + 1).to_string().bold(),
                    action.description,
                    safety_badge
                );
                println!("   {} {}", "Command:".dimmed(), action.command.cyan());
                println!();
            }

            println!(
                "{} Run {} to execute this plan.",
                "→".cyan(),
                "zenvo repair --apply".cyan()
            );
        }
    }

    if apply {
        let mut executed = Vec::new();
        let mut skipped = Vec::new();
        let mut failed = Vec::new();

        if format == OutputFormat::Text {
            println!("{}", "Executing Repair Plan".bold().cyan());
            println!("{}", "═".repeat(50).dimmed());
            println!();
        }

        for action in &repair_plan {
            if format == OutputFormat::Text {
                println!("{} {}", "→".cyan(), action.description);
            }

            // Confirm if not safe and not auto-yes (only in text mode)
            if !action.is_safe && !auto_yes && format == OutputFormat::Text {
                print!("  Execute {}? [y/N] ", action.command.cyan());
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                if !input.trim().eq_ignore_ascii_case("y") {
                    if format == OutputFormat::Text {
                        println!("  {}", "Skipped".yellow());
                    }
                    skipped.push(action.description.clone());
                    continue;
                }
            }

            // In JSON mode with auto_yes=false, skip non-safe actions
            if !action.is_safe && !auto_yes && format == OutputFormat::Json {
                skipped.push(action.description.clone());
                continue;
            }

            // Execute
            match execute_repair(action) {
                Ok(_) => {
                    if format == OutputFormat::Text {
                        println!("  {}", "Done".green());
                    }
                    executed.push(action.description.clone());
                }
                Err(e) => {
                    if format == OutputFormat::Text {
                        println!("  {} {}", "Failed:".red(), e);
                    }
                    failed.push(serde_json::json!({
                        "action": action.description,
                        "error": e.to_string()
                    }));
                }
            }
        }

        if format == OutputFormat::Json {
            let output = ZenvoOutput::new("repair")
                .with_success(failed.is_empty())
                .with_data(serde_json::json!({
                    "executed": executed,
                    "skipped": skipped,
                    "failed": failed,
                    "total": repair_plan.len()
                }));

            println!("{}", output.to_json()?);
        } else {
            println!();
            println!(
                "{}",
                "Repair complete. Run `zenvo doctor` to verify.".green()
            );
        }
    }

    Ok(())
}
