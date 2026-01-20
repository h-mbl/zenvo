use clap::{Parser, Subcommand};
use colored::Colorize;

mod checks;
mod commands;
mod config;
mod lockfile;
mod output;
mod repair;
mod utils;

use commands::config::ConfigAction;

pub use output::OutputFormat;

/// Zenvo - Node.js environment lock, doctor & repair tool
/// Generates env.lock, detects drift, and provides guided repair.
#[derive(Parser)]
#[command(name = "zenvo")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Output format (text, json)
    #[arg(long, global = true, default_value = "text")]
    format: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize zenvo in the current project
    Init {
        /// Force overwrite existing env.lock
        #[arg(short, long)]
        force: bool,
    },

    /// Generate or update env.lock file
    Lock {
        /// Include optional metadata (OS, arch)
        #[arg(long)]
        full: bool,
    },

    /// Run diagnostic checks on the environment
    Doctor {
        /// Only check specific category
        #[arg(short, long, value_enum)]
        category: Option<checks::CheckCategory>,
    },

    /// Show repair plan or apply fixes
    Repair {
        /// Show plan without executing
        #[arg(long)]
        plan: bool,

        /// Apply the repair plan
        #[arg(long)]
        apply: bool,

        /// Auto-approve safe repairs
        #[arg(short, long)]
        yes: bool,
    },

    /// Verify environment matches env.lock
    Verify {
        /// Exit with error on any drift (errors + warnings)
        #[arg(long)]
        strict: bool,

        /// Print warnings but exit 0
        #[arg(long)]
        warn: bool,
    },

    /// Show current environment status
    Status,

    /// Show diff between current and locked state
    Diff,

    /// Clean caches safely
    Clean {
        /// What to clean: node_modules, npm-cache, all
        #[arg(default_value = "all")]
        target: String,

        /// Actually delete (default is dry-run)
        #[arg(long)]
        force: bool,
    },

    /// Guided dependency upgrade
    Upgrade {
        /// Interactive mode - confirm each upgrade
        #[arg(short, long)]
        interactive: bool,

        /// Include major version upgrades
        #[arg(long)]
        major: bool,

        /// Show plan without executing
        #[arg(long)]
        dry_run: bool,
    },

    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },

    /// Search for available package versions on npm registry
    Versions {
        /// Package name to search (e.g., "express", "@types/node", "expo-notifications")
        package: String,

        /// Show only versions compatible with a constraint (e.g., "^18.0.0", "~0.31")
        #[arg(short, long)]
        constraint: Option<String>,

        /// Number of recent versions to show
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Show all versions (ignore limit)
        #[arg(long)]
        all: bool,
    },

    /// Detect and resolve dependency conflicts automatically
    Resolve {
        /// Show what would be changed without applying
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Create default .env.doctor.toml configuration file
    Init {
        /// Force overwrite existing config
        #[arg(short, long)]
        force: bool,
    },

    /// Validate configuration file
    Validate,
}

fn main() {
    let cli = Cli::parse();
    let format = OutputFormat::from_str(&cli.format);
    let is_json = format == OutputFormat::Json;

    if !is_json {
        println!("{}", "⚡ Zenvo".bold().cyan());
        println!("{}", "Node.js Environment Lock & Doctor".dimmed());
        println!();
    }

    let result = match cli.command {
        Commands::Init { force } => commands::init::run(force, format),
        Commands::Lock { full } => commands::lock::run(full, format),
        Commands::Doctor { category } => commands::doctor::run(category, format),
        Commands::Repair { plan, apply, yes } => commands::repair::run(plan, apply, yes, format),
        Commands::Verify { strict, warn } => commands::verify::run(strict, warn, format),
        Commands::Status => commands::status::run(format),
        Commands::Diff => commands::diff::run(format),
        Commands::Clean { target, force } => commands::clean::run(target, force, format),
        Commands::Upgrade {
            interactive,
            major,
            dry_run,
        } => commands::upgrade::run(interactive, major, dry_run, format),
        Commands::Config { action } => {
            let config_action = match action {
                ConfigCommands::Init { force } => ConfigAction::Init { force },
                ConfigCommands::Validate => ConfigAction::Validate,
            };
            commands::config::run(config_action, format)
        }
        Commands::Versions {
            package,
            constraint,
            limit,
            all,
        } => commands::versions::run(&package, constraint.as_deref(), limit, all, format),
        Commands::Resolve { dry_run } => commands::resolve::run(dry_run, format),
    };

    if let Err(e) = result {
        if is_json {
            let error_output = serde_json::json!({
                "success": false,
                "error": e.to_string(),
                "timestamp": chrono::Utc::now().to_rfc3339()
            });
            eprintln!("{}", serde_json::to_string_pretty(&error_output).unwrap_or_default());
        } else {
            eprintln!("{} {}", "Error:".red().bold(), e);
        }
        std::process::exit(1);
    }
}
