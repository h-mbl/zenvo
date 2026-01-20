use serde::Serialize;
use chrono::Utc;

use crate::checks::{CheckResult, CheckSeverity, CurrentEnvironment};

/// Output format for CLI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => OutputFormat::Json,
            _ => OutputFormat::Text,
        }
    }
}

/// Standardized output structure for all Zenvo commands
#[derive(Debug, Clone, Serialize)]
pub struct ZenvoOutput {
    pub command: String,
    pub success: bool,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift_detected: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<Issue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ZenvoOutput {
    pub fn new(command: &str) -> Self {
        Self {
            command: command.to_string(),
            success: true,
            timestamp: Utc::now().to_rfc3339(),
            drift_detected: None,
            issues: Vec::new(),
            environment: None,
            data: None,
        }
    }

    pub fn with_success(mut self, success: bool) -> Self {
        self.success = success;
        self
    }

    pub fn with_drift(mut self, detected: bool) -> Self {
        self.drift_detected = Some(detected);
        self
    }

    pub fn with_issues(mut self, issues: Vec<Issue>) -> Self {
        self.issues = issues;
        self
    }

    pub fn with_environment(mut self, env: EnvironmentStatus) -> Self {
        self.environment = Some(env);
        self
    }

    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

/// Issue representation for JSON output
#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub name: String,
    pub category: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_fix: Option<String>,
}

impl From<&CheckResult> for Issue {
    fn from(result: &CheckResult) -> Self {
        Self {
            name: result.name.clone(),
            category: result.category.clone(),
            severity: match result.severity {
                CheckSeverity::Pass => "pass".to_string(),
                CheckSeverity::Info => "info".to_string(),
                CheckSeverity::Warning => "warning".to_string(),
                CheckSeverity::Error => "error".to_string(),
            },
            message: result.message.clone(),
            suggested_fix: result.suggested_fix.clone(),
        }
    }
}

/// Environment status for JSON output
#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentStatus {
    pub node_version: String,
    pub package_manager: String,
    pub package_manager_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lockfile_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lockfile_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_env_lock: Option<bool>,
}

impl From<&CurrentEnvironment> for EnvironmentStatus {
    fn from(env: &CurrentEnvironment) -> Self {
        Self {
            node_version: env.node_version.clone(),
            package_manager: env.package_manager.clone(),
            package_manager_version: env.package_manager_version.clone(),
            lockfile_type: env.lockfile_type.clone(),
            lockfile_hash: env.lockfile_hash.clone(),
            has_env_lock: None,
        }
    }
}

impl EnvironmentStatus {
    pub fn with_env_lock(mut self, has_lock: bool) -> Self {
        self.has_env_lock = Some(has_lock);
        self
    }
}

/// Diff item for JSON output
#[derive(Debug, Clone, Serialize)]
pub struct DiffItem {
    pub field: String,
    pub locked: String,
    pub current: String,
    pub matches: bool,
}

/// Diff output structure
#[derive(Debug, Clone, Serialize)]
pub struct DiffOutput {
    pub items: Vec<DiffItem>,
    pub has_drift: bool,
}

/// Repair action for JSON output
#[derive(Debug, Clone, Serialize)]
pub struct RepairActionJson {
    pub description: String,
    pub command: String,
    pub is_safe: bool,
}

/// Repair plan output
#[derive(Debug, Clone, Serialize)]
pub struct RepairPlanOutput {
    pub actions: Vec<RepairActionJson>,
    pub total_issues: usize,
    pub safe_actions: usize,
    pub review_actions: usize,
}

/// Clean target info for JSON output
#[derive(Debug, Clone, Serialize)]
pub struct CleanTarget {
    pub path: String,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub exists: bool,
}

/// Clean output structure
#[derive(Debug, Clone, Serialize)]
pub struct CleanOutput {
    pub targets: Vec<CleanTarget>,
    pub total_size_bytes: u64,
    pub total_size_formatted: String,
    pub dry_run: bool,
}
