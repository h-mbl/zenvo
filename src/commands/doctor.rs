use anyhow::Result;
use colored::Colorize;

use crate::checks::{CheckCategory, CheckResult, CheckSeverity, run_all_checks};
use crate::config::ZenvoConfig;
use crate::lockfile::EnvLock;
use crate::output::{Issue, OutputFormat, ZenvoOutput};

pub fn run(category: Option<CheckCategory>, format: OutputFormat) -> Result<()> {
    if format == OutputFormat::Text {
        println!("{}", "Running environment checks...".cyan());
        println!();
    }

    // Load env.lock if it exists
    let env_lock = EnvLock::load_if_exists()?;

    // Load config if it exists
    let config = ZenvoConfig::load_if_exists()?;

    // Run checks
    let results = run_all_checks(&env_lock, category, &config)?;

    // Count issues
    let has_errors = results.iter().any(|r| r.severity == CheckSeverity::Error);
    let has_warnings = results.iter().any(|r| r.severity == CheckSeverity::Warning);

    // Output results
    match format {
        OutputFormat::Json => output_json(&results, has_errors, has_warnings)?,
        OutputFormat::Text => output_text(&results),
    }

    // Exit with error if any critical issues
    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}

fn output_text(results: &[CheckResult]) {
    let mut errors = 0;
    let mut warnings = 0;
    let mut passed = 0;

    for result in results {
        let (icon, color) = match result.severity {
            CheckSeverity::Pass => {
                passed += 1;
                ("✓", "green")
            }
            CheckSeverity::Warning => {
                warnings += 1;
                ("⚠", "yellow")
            }
            CheckSeverity::Error => {
                errors += 1;
                ("✗", "red")
            }
            CheckSeverity::Info => ("ℹ", "blue"),
        };

        let icon_colored = match color {
            "green" => icon.green(),
            "yellow" => icon.yellow(),
            "red" => icon.red(),
            _ => icon.blue(),
        };

        println!("{} {}", icon_colored, result.name);
        
        if !result.message.is_empty() {
            println!("  {}", result.message.dimmed());
        }

        if let Some(ref fix) = result.suggested_fix {
            println!("  {} {}", "Fix:".cyan(), fix);
        }
    }

    println!();
    println!(
        "{}: {} passed, {} warnings, {} errors",
        "Summary".bold(),
        passed.to_string().green(),
        warnings.to_string().yellow(),
        errors.to_string().red()
    );

    if errors == 0 && warnings == 0 {
        println!();
        println!("{}", "Environment is healthy! ✨".green().bold());
    } else if errors > 0 {
        println!();
        println!(
            "{} Run {} to see repair options.",
            "→".cyan(),
            "zenvo repair --plan".cyan()
        );
    }
}

fn output_json(results: &[CheckResult], has_errors: bool, has_warnings: bool) -> Result<()> {
    let issues: Vec<Issue> = results.iter().map(Issue::from).collect();

    let errors = results.iter().filter(|r| r.severity == CheckSeverity::Error).count();
    let warnings = results.iter().filter(|r| r.severity == CheckSeverity::Warning).count();
    let passed = results.iter().filter(|r| r.severity == CheckSeverity::Pass).count();

    let output = ZenvoOutput::new("doctor")
        .with_success(!has_errors)
        .with_drift(has_errors || has_warnings)
        .with_issues(issues)
        .with_data(serde_json::json!({
            "summary": {
                "total": results.len(),
                "passed": passed,
                "warnings": warnings,
                "errors": errors
            }
        }));

    println!("{}", output.to_json()?);
    Ok(())
}
