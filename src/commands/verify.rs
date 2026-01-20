use anyhow::Result;
use colored::Colorize;

use crate::checks::{run_all_checks, CheckSeverity};
use crate::config::ZenvoConfig;
use crate::lockfile::EnvLock;
use crate::output::{Issue, OutputFormat, ZenvoOutput};

/// Run verify command
///
/// Modes:
/// - Default: Exit 1 on errors, exit 0 on warnings only
/// - --strict: Exit 1 on errors OR warnings
/// - --warn: Exit 0 always, but print warnings/errors (don't fail CI)
pub fn run(strict: bool, warn_only: bool, format: OutputFormat) -> Result<()> {
    // Load env.lock (required for verify)
    let env_lock = EnvLock::load()?;

    // Load config if it exists
    let config = ZenvoConfig::load_if_exists()?;

    // Run all checks
    let results = run_all_checks(&Some(env_lock), None, &config)?;

    let errors: Vec<_> = results
        .iter()
        .filter(|r| r.severity == CheckSeverity::Error)
        .collect();

    let warnings: Vec<_> = results
        .iter()
        .filter(|r| r.severity == CheckSeverity::Warning)
        .collect();

    let has_drift = !errors.is_empty() || !warnings.is_empty();

    // Determine exit behavior based on mode:
    // - warn_only: always exit 0 (just print warnings)
    // - strict: exit 1 if any errors OR warnings
    // - default: exit 1 only if errors
    let should_fail = if warn_only {
        false // Never fail in warn mode
    } else if strict {
        !errors.is_empty() || !warnings.is_empty()
    } else {
        !errors.is_empty()
    };

    let passed = !should_fail;

    if format == OutputFormat::Json {
        let issues: Vec<Issue> = results
            .iter()
            .filter(|r| r.severity == CheckSeverity::Error || r.severity == CheckSeverity::Warning)
            .map(Issue::from)
            .collect();

        let output = ZenvoOutput::new("verify")
            .with_success(passed)
            .with_drift(has_drift)
            .with_issues(issues)
            .with_data(serde_json::json!({
                "strict": strict,
                "warn_only": warn_only,
                "errors": errors.len(),
                "warnings": warnings.len(),
                "passed": results.iter().filter(|r| r.severity == CheckSeverity::Pass).count()
            }));

        println!("{}", output.to_json()?);

        if should_fail {
            std::process::exit(1);
        }
    } else {
        // CI-friendly output
        if !has_drift {
            println!("{} Environment matches env.lock", "✓".green().bold());
            std::process::exit(0);
        }

        // Has drift - print issues
        if warn_only {
            // Warn mode: print as warnings, don't fail
            println!(
                "{} Environment drift detected (warn mode - not failing)",
                "⚠".yellow().bold()
            );
        } else {
            println!("{} Environment drift detected!", "✗".red().bold());
        }
        println!();

        for result in &errors {
            println!(
                "{} {}: {}",
                "ERROR".red().bold(),
                result.name,
                result.message
            );
        }

        // Print warnings in strict mode or warn mode
        if strict || warn_only || !errors.is_empty() {
            for result in &warnings {
                println!(
                    "{} {}: {}",
                    "WARN".yellow().bold(),
                    result.name,
                    result.message
                );
            }
        }

        if should_fail {
            println!();
            println!(
                "Run {} locally to fix issues.",
                "zenvo repair --apply".cyan()
            );
            std::process::exit(1);
        } else if warn_only && has_drift {
            println!();
            println!(
                "{} Consider running {} locally.",
                "→".cyan(),
                "zenvo repair --apply".cyan()
            );
        }
    }

    Ok(())
}
