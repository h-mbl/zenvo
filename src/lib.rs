//! Zenvo - Node.js Environment Lock & Doctor
//! This library provides the core functionality for Zenvo, enabling programmatic
//! access to environment detection, locking, and repair capabilities.

pub mod checks;
pub mod config;
pub mod lockfile;
pub mod mcp;
pub mod output;
pub mod repair;
pub mod utils;

// Re-export main types for convenience
pub use checks::{
    detect_current_environment, run_all_checks, CheckCategory, CheckResult, CheckSeverity,
    CurrentEnvironment,
};
pub use config::ZenvoConfig;
pub use lockfile::EnvLock;
pub use output::{
    CleanOutput, CleanTarget, DiffItem, DiffOutput, EnvironmentStatus, Issue, OutputFormat,
    RepairActionJson, RepairPlanOutput, ZenvoOutput,
};
pub use repair::{
    execute_repair, generate_repair_plan_with_context, RepairAction, RepairContext,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
