//! Configuration module for Zenvo
//! Handles loading and parsing of `.env.doctor.toml` configuration files.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::checks::CheckSeverity;

/// Default configuration file name
pub const CONFIG_FILE: &str = ".env.doctor.toml";

/// Main configuration structure for Zenvo
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ZenvoConfig {
    /// Policy settings for environment management
    #[serde(default)]
    pub policies: Policies,

    /// Check configuration (disabled checks, severity overrides)
    #[serde(default)]
    pub checks: ChecksConfig,

    /// Framework-specific settings
    #[serde(default)]
    pub frameworks: FrameworksConfig,
}

/// Policy settings that control Zenvo behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policies {
    /// Allow minor version upgrades of Node.js
    #[serde(default = "default_true")]
    pub allow_node_upgrade_minor: bool,

    /// Allow major version upgrades of Node.js
    #[serde(default)]
    pub allow_node_upgrade_major: bool,

    /// Require lockfile to be frozen (no modifications allowed)
    #[serde(default = "default_true")]
    pub require_lockfile_frozen: bool,

    /// Enforce corepack usage
    #[serde(default)]
    pub enforce_corepack: bool,

    /// Allowed package managers (empty = all allowed)
    #[serde(default)]
    pub allowed_package_managers: Vec<String>,

    /// Minimum Node.js version required
    #[serde(default)]
    pub min_node_version: Option<String>,

    /// Maximum Node.js version allowed
    #[serde(default)]
    pub max_node_version: Option<String>,
}

impl Default for Policies {
    fn default() -> Self {
        Self {
            allow_node_upgrade_minor: true,
            allow_node_upgrade_major: false,
            require_lockfile_frozen: true,
            enforce_corepack: false,
            allowed_package_managers: Vec::new(),
            min_node_version: None,
            max_node_version: None,
        }
    }
}

/// Configuration for checks
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChecksConfig {
    /// List of check names to disable
    #[serde(default)]
    pub disabled: Vec<String>,

    /// Override severity for specific checks
    #[serde(default)]
    pub severity_overrides: HashMap<String, SeverityOverride>,

    /// Custom check timeout in seconds
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

/// Severity override configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeverityOverride {
    Pass,
    Info,
    Warning,
    Error,
}

impl From<SeverityOverride> for CheckSeverity {
    fn from(override_val: SeverityOverride) -> Self {
        match override_val {
            SeverityOverride::Pass => CheckSeverity::Pass,
            SeverityOverride::Info => CheckSeverity::Info,
            SeverityOverride::Warning => CheckSeverity::Warning,
            SeverityOverride::Error => CheckSeverity::Error,
        }
    }
}

/// Framework-specific configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrameworksConfig {
    /// Next.js specific settings
    #[serde(default)]
    pub nextjs: NextjsConfig,

    /// React specific settings
    #[serde(default)]
    pub react: ReactConfig,

    /// TypeScript specific settings
    #[serde(default)]
    pub typescript: TypeScriptConfig,
}

/// Next.js configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NextjsConfig {
    /// Require specific Next.js version
    #[serde(default)]
    pub required_version: Option<String>,

    /// Check .next cache integrity
    #[serde(default = "default_true")]
    pub check_cache_integrity: bool,
}

/// React configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReactConfig {
    /// Enforce React/ReactDOM version match
    #[serde(default = "default_true")]
    pub enforce_version_match: bool,
}

/// TypeScript configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeScriptConfig {
    /// Require tsconfig.json
    #[serde(default = "default_true")]
    pub require_tsconfig: bool,

    /// Check for strict mode
    #[serde(default)]
    pub enforce_strict: bool,
}

fn default_true() -> bool {
    true
}

impl ZenvoConfig {
    /// Load configuration from the default location
    pub fn load() -> Result<Self> {
        Self::load_from(Path::new(CONFIG_FILE))
    }

    /// Load configuration from a specific path
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: ZenvoConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    /// Load configuration if file exists, otherwise return None
    pub fn load_if_exists() -> Result<Option<Self>> {
        let path = Path::new(CONFIG_FILE);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(Self::load()?))
    }

    /// Save configuration to file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;

        let header = "# Zenvo Configuration\n# See https://github.com/zenvo/zenvo for documentation\n\n";
        let full_content = format!("{}{}", header, content);

        fs::write(path, full_content)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;

        Ok(())
    }

    /// Create a default configuration file
    pub fn create_default(path: &Path) -> Result<Self> {
        let config = Self::default();
        config.save(path)?;
        Ok(config)
    }

    /// Check if a specific check is disabled
    pub fn is_check_disabled(&self, check_name: &str) -> bool {
        self.checks
            .disabled
            .iter()
            .any(|name| name.eq_ignore_ascii_case(check_name))
    }

    /// Get severity override for a check
    pub fn get_severity_override(&self, check_name: &str) -> Option<CheckSeverity> {
        self.checks
            .severity_overrides
            .get(check_name)
            .map(|s| s.clone().into())
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate min/max node versions
        if let (Some(min), Some(max)) = (&self.policies.min_node_version, &self.policies.max_node_version) {
            let min_ver = semver::Version::parse(min)
                .with_context(|| format!("Invalid min_node_version: {}", min))?;
            let max_ver = semver::Version::parse(max)
                .with_context(|| format!("Invalid max_node_version: {}", max))?;

            if min_ver > max_ver {
                anyhow::bail!(
                    "min_node_version ({}) is greater than max_node_version ({})",
                    min,
                    max
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ZenvoConfig::default();
        assert!(config.policies.allow_node_upgrade_minor);
        assert!(!config.policies.allow_node_upgrade_major);
        assert!(config.policies.require_lockfile_frozen);
    }

    #[test]
    fn test_parse_config() {
        let toml_content = r#"
[policies]
allow_node_upgrade_minor = false
enforce_corepack = true

[checks]
disabled = ["deprecated_packages"]

[checks.severity_overrides]
"peer_dependencies" = "warning"
"#;

        let config: ZenvoConfig = toml::from_str(toml_content).unwrap();
        assert!(!config.policies.allow_node_upgrade_minor);
        assert!(config.policies.enforce_corepack);
        assert!(config.is_check_disabled("deprecated_packages"));
    }
}
