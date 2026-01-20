use anyhow::Result;
use colored::Colorize;

use crate::checks::detect_current_environment;
use crate::lockfile::EnvLock;
use crate::output::{DiffItem, DiffOutput, OutputFormat, ZenvoOutput};

pub fn run(format: OutputFormat) -> Result<()> {
    let locked = EnvLock::load()?;
    let current = detect_current_environment()?;

    // Build diff items
    let mut diff_items = Vec::new();

    // Node.js
    let node_match = current.node_version == locked.toolchain.node;
    diff_items.push(DiffItem {
        field: "Node.js".to_string(),
        locked: locked.toolchain.node.clone(),
        current: current.node_version.clone(),
        matches: node_match,
    });

    // Package Manager
    let pm_match = current.package_manager == locked.toolchain.package_manager;
    diff_items.push(DiffItem {
        field: "Package Manager".to_string(),
        locked: locked.toolchain.package_manager.clone(),
        current: current.package_manager.clone(),
        matches: pm_match,
    });

    // PM Version
    let pmv_match = current.package_manager_version == locked.toolchain.package_manager_version;
    diff_items.push(DiffItem {
        field: "PM Version".to_string(),
        locked: locked.toolchain.package_manager_version.clone(),
        current: current.package_manager_version.clone(),
        matches: pmv_match,
    });

    // Lockfile hash
    if let Some(ref lockfile_info) = locked.lockfile {
        let current_hash = current.lockfile_hash.as_deref().unwrap_or("N/A");
        let hash_match = current_hash == lockfile_info.hash;
        diff_items.push(DiffItem {
            field: "Lockfile Hash".to_string(),
            locked: lockfile_info.hash.chars().take(12).collect(),
            current: current_hash.chars().take(12).collect(),
            matches: hash_match,
        });
    }

    let has_drift = diff_items.iter().any(|item| !item.matches);

    if format == OutputFormat::Json {
        let diff_output = DiffOutput {
            items: diff_items,
            has_drift,
        };

        let output = ZenvoOutput::new("diff")
            .with_success(true)
            .with_drift(has_drift)
            .with_data(serde_json::to_value(&diff_output)?);

        println!("{}", output.to_json()?);
    } else {
        println!("{}", "Environment Diff".bold().cyan());
        println!("{}", "═".repeat(50).dimmed());
        println!();
        println!(
            "{:20} {:20} {:20}",
            "".bold(),
            "LOCKED".dimmed(),
            "CURRENT".dimmed()
        );
        println!("{}", "─".repeat(60).dimmed());

        for item in &diff_items {
            print_diff_line(&item.field, &item.locked, &item.current, item.matches);
        }

        println!();
    }

    Ok(())
}

fn print_diff_line(label: &str, locked: &str, current: &str, matches: bool) {
    let status = if matches {
        "=".green().to_string()
    } else {
        "≠".red().to_string()
    };

    let current_display = if matches {
        current.to_string()
    } else {
        current.red().to_string()
    };

    println!(
        "{:20} {:20} {:20} {}",
        label.bold(),
        locked.dimmed(),
        current_display,
        status
    );
}
