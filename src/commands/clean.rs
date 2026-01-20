use anyhow::Result;
use colored::Colorize;
use std::fs;
use std::path::Path;

use crate::output::{CleanOutput, CleanTarget, OutputFormat, ZenvoOutput};

pub fn run(target: String, force: bool, format: OutputFormat) -> Result<()> {
    let targets: Vec<&str> = match target.as_str() {
        "all" => vec!["node_modules", "npm-cache", ".next", ".turbo", ".vite"],
        "node_modules" => vec!["node_modules"],
        "npm-cache" => vec!["npm-cache"],
        "build" => vec![".next", ".turbo", ".vite", "dist", "build"],
        _ => {
            if format == OutputFormat::Json {
                let output = ZenvoOutput::new("clean")
                    .with_success(false)
                    .with_data(serde_json::json!({
                        "error": format!("Unknown target: {}", target),
                        "available": ["all", "node_modules", "npm-cache", "build"]
                    }));
                println!("{}", output.to_json()?);
            } else {
                println!("{} Unknown target: {}", "Error:".red(), target);
                println!("Available: all, node_modules, npm-cache, build");
            }
            return Ok(());
        }
    };

    if format == OutputFormat::Text {
        if !force {
            println!("{}", "Dry run - would clean:".cyan().bold());
        } else {
            println!("{}", "Cleaning...".cyan().bold());
        }
        println!();
    }

    let mut total_size: u64 = 0;
    let mut clean_targets: Vec<CleanTarget> = Vec::new();
    let mut cleaned: Vec<String> = Vec::new();
    let mut failed: Vec<serde_json::Value> = Vec::new();

    for t in &targets {
        let path = Path::new(t);
        if path.exists() {
            let size = dir_size(path).unwrap_or(0);
            total_size += size;

            clean_targets.push(CleanTarget {
                path: t.to_string(),
                size_bytes: size,
                size_formatted: format_size(size),
                exists: true,
            });

            if force {
                match fs::remove_dir_all(path) {
                    Ok(_) => {
                        if format == OutputFormat::Text {
                            println!(
                                "  {} {} ({})",
                                "✓".green(),
                                t,
                                format_size(size).dimmed()
                            );
                        }
                        cleaned.push(t.to_string());
                    }
                    Err(e) => {
                        if format == OutputFormat::Text {
                            println!("  {} {} - {}", "✗".red(), t, e);
                        }
                        failed.push(serde_json::json!({
                            "path": t,
                            "error": e.to_string()
                        }));
                    }
                }
            } else if format == OutputFormat::Text {
                println!("  {} {} ({})", "→".cyan(), t, format_size(size).dimmed());
            }
        }
    }

    // Also check npm cache
    if targets.contains(&"npm-cache") {
        if let Some(cache_dir) = dirs::cache_dir() {
            let npm_cache = cache_dir.join("npm");
            if npm_cache.exists() {
                let size = dir_size(&npm_cache).unwrap_or(0);
                total_size += size;

                clean_targets.push(CleanTarget {
                    path: npm_cache.to_string_lossy().to_string(),
                    size_bytes: size,
                    size_formatted: format_size(size),
                    exists: true,
                });

                if format == OutputFormat::Text {
                    if force {
                        // Use npm cache clean instead of deleting directly
                        println!(
                            "  {} npm cache (run `npm cache clean --force` manually)",
                            "⚠".yellow()
                        );
                    } else {
                        println!(
                            "  {} npm cache ({})",
                            "→".cyan(),
                            format_size(size).dimmed()
                        );
                    }
                }
            }
        }
    }

    if format == OutputFormat::Json {
        let clean_output = CleanOutput {
            targets: clean_targets,
            total_size_bytes: total_size,
            total_size_formatted: format_size(total_size),
            dry_run: !force,
        };

        let mut data = serde_json::to_value(&clean_output)?;
        if force {
            data["cleaned"] = serde_json::json!(cleaned);
            data["failed"] = serde_json::json!(failed);
        }

        let output = ZenvoOutput::new("clean")
            .with_success(failed.is_empty())
            .with_data(data);

        println!("{}", output.to_json()?);
    } else {
        println!();
        println!("Total: {}", format_size(total_size).bold());

        if !force {
            println!();
            println!("Run {} to actually delete.", "zenvo clean --force".cyan());
        }
    }

    Ok(())
}

/// Calculate directory size with a depth limit for performance
/// For node_modules, we limit depth to avoid excessive traversal
const MAX_DEPTH_FOR_SIZE_CALC: usize = 10;

fn dir_size(path: &Path) -> Result<u64> {
    let mut size = 0;
    if path.is_dir() {
        // Use max_depth to prevent extremely deep traversal in large node_modules
        for entry in walkdir::WalkDir::new(path)
            .max_depth(MAX_DEPTH_FOR_SIZE_CALC)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                size += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    Ok(size)
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
