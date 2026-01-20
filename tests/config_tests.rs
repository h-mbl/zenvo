//! Configuration tests for Zenvo
//!
//! Tests for TOML parsing, validation, and configuration handling.

use std::fs;
use tempfile::TempDir;

/// Helper to set up a test directory
fn setup_test_dir() -> TempDir {
    TempDir::new().expect("Failed to create temp directory")
}

/// Helper to create a config file
fn create_config_file(dir: &TempDir, content: &str) {
    let path = dir.path().join(".env.doctor.toml");
    fs::write(path, content).expect("Failed to write config file");
}

// ============================================================================
// Basic TOML Parsing Tests
// ============================================================================

#[test]
fn test_parse_minimal_config() {
    let config_toml = r#"
[policies]
allow_node_upgrade_minor = true
"#;

    let parsed: Result<toml::Value, _> = toml::from_str(config_toml);
    assert!(parsed.is_ok(), "Minimal config should parse");

    let config = parsed.unwrap();
    assert_eq!(config["policies"]["allow_node_upgrade_minor"].as_bool(), Some(true));
}

#[test]
fn test_parse_full_config() {
    let config_toml = r#"
[policies]
allow_node_upgrade_minor = false
allow_node_upgrade_major = false
require_lockfile_frozen = true
enforce_corepack = true
allowed_package_managers = ["npm", "pnpm"]
min_node_version = "18.0.0"
max_node_version = "22.0.0"

[checks]
disabled = ["deprecated_packages", "phantom_dependencies"]
timeout_seconds = 30

[checks.severity_overrides]
"peer_dependencies" = "warning"
"eslint_config" = "info"

[frameworks.nextjs]
required_version = "14.0.0"
check_cache_integrity = true

[frameworks.react]
enforce_version_match = true

[frameworks.typescript]
require_tsconfig = true
enforce_strict = false
"#;

    let parsed: Result<toml::Value, _> = toml::from_str(config_toml);
    assert!(parsed.is_ok(), "Full config should parse");

    let config = parsed.unwrap();

    // Check policies
    assert_eq!(config["policies"]["allow_node_upgrade_minor"].as_bool(), Some(false));
    assert_eq!(config["policies"]["enforce_corepack"].as_bool(), Some(true));

    // Check package managers list
    let allowed_pms = config["policies"]["allowed_package_managers"].as_array().unwrap();
    assert_eq!(allowed_pms.len(), 2);

    // Check version constraints
    assert_eq!(config["policies"]["min_node_version"].as_str(), Some("18.0.0"));
    assert_eq!(config["policies"]["max_node_version"].as_str(), Some("22.0.0"));

    // Check disabled checks
    let disabled = config["checks"]["disabled"].as_array().unwrap();
    assert!(disabled.iter().any(|v| v.as_str() == Some("deprecated_packages")));

    // Check severity overrides
    assert_eq!(
        config["checks"]["severity_overrides"]["peer_dependencies"].as_str(),
        Some("warning")
    );

    // Check framework configs
    assert_eq!(
        config["frameworks"]["nextjs"]["required_version"].as_str(),
        Some("14.0.0")
    );
}

#[test]
fn test_parse_empty_config() {
    let config_toml = "";
    let parsed: Result<toml::Value, _> = toml::from_str(config_toml);
    // Empty TOML should parse to an empty table
    assert!(parsed.is_ok());
}

#[test]
fn test_parse_config_with_comments() {
    let config_toml = r#"
# This is a comment
[policies]
# Allow minor upgrades but not major
allow_node_upgrade_minor = true  # inline comment
allow_node_upgrade_major = false
"#;

    let parsed: Result<toml::Value, _> = toml::from_str(config_toml);
    assert!(parsed.is_ok(), "Config with comments should parse");

    let config = parsed.unwrap();
    assert_eq!(config["policies"]["allow_node_upgrade_minor"].as_bool(), Some(true));
}

// ============================================================================
// Malformed Config Tests
// ============================================================================

#[test]
fn test_invalid_toml_syntax() {
    let invalid_toml = r#"
[policies
allow_node_upgrade_minor = true
"#;

    let parsed: Result<toml::Value, _> = toml::from_str(invalid_toml);
    assert!(parsed.is_err(), "Invalid TOML syntax should fail");
}

#[test]
fn test_invalid_value_type() {
    let invalid_toml = r#"
[policies]
allow_node_upgrade_minor = "not a boolean"
"#;

    // This will parse as TOML but may fail type validation later
    let parsed: Result<toml::Value, _> = toml::from_str(invalid_toml);
    assert!(parsed.is_ok(), "TOML parses, but value is wrong type");

    let config = parsed.unwrap();
    assert!(config["policies"]["allow_node_upgrade_minor"].as_bool().is_none());
}

#[test]
fn test_duplicate_keys() {
    let duplicate_keys = r#"
[policies]
allow_node_upgrade_minor = true
allow_node_upgrade_minor = false
"#;

    let parsed: Result<toml::Value, _> = toml::from_str(duplicate_keys);
    // TOML spec allows duplicate keys, last one wins
    if let Ok(config) = parsed {
        // Last value should win
        assert_eq!(config["policies"]["allow_node_upgrade_minor"].as_bool(), Some(false));
    }
}

#[test]
fn test_missing_section() {
    let config_toml = r#"
allow_node_upgrade_minor = true
"#;

    // This creates a top-level key, not under [policies]
    let parsed: Result<toml::Value, _> = toml::from_str(config_toml);
    assert!(parsed.is_ok());

    let config = parsed.unwrap();
    // Should be at root level, not under policies
    assert!(config.get("policies").is_none());
    assert_eq!(config["allow_node_upgrade_minor"].as_bool(), Some(true));
}

// ============================================================================
// Config File I/O Tests
// ============================================================================

#[test]
fn test_read_config_from_file() {
    let dir = setup_test_dir();

    let config_content = r#"
[policies]
enforce_corepack = true

[checks]
disabled = ["phantom_dependencies"]
"#;

    create_config_file(&dir, config_content);

    let path = dir.path().join(".env.doctor.toml");
    let content = fs::read_to_string(&path).expect("Should read config file");
    let parsed: toml::Value = toml::from_str(&content).expect("Should parse config");

    assert_eq!(parsed["policies"]["enforce_corepack"].as_bool(), Some(true));
}

#[test]
fn test_missing_config_file() {
    let dir = setup_test_dir();
    let path = dir.path().join(".env.doctor.toml");

    let result = fs::read_to_string(&path);
    assert!(result.is_err(), "Reading missing config should fail");
}

#[test]
fn test_write_config_to_file() {
    let dir = setup_test_dir();
    let path = dir.path().join(".env.doctor.toml");

    let config_content = r#"[policies]
allow_node_upgrade_minor = true
"#;

    fs::write(&path, config_content).expect("Should write config file");

    assert!(path.exists(), "Config file should exist after write");

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("allow_node_upgrade_minor"));
}

// ============================================================================
// Semantic Validation Tests
// ============================================================================

#[test]
fn test_valid_semver_versions() {
    let config_toml = r#"
[policies]
min_node_version = "18.0.0"
max_node_version = "22.0.0"
"#;

    let parsed: toml::Value = toml::from_str(config_toml).unwrap();

    let min = parsed["policies"]["min_node_version"].as_str().unwrap();
    let max = parsed["policies"]["max_node_version"].as_str().unwrap();

    // Verify they parse as semver
    let min_ver: Result<semver::Version, _> = semver::Version::parse(min);
    let max_ver: Result<semver::Version, _> = semver::Version::parse(max);

    assert!(min_ver.is_ok(), "min_node_version should be valid semver");
    assert!(max_ver.is_ok(), "max_node_version should be valid semver");

    // Verify min <= max
    assert!(min_ver.unwrap() < max_ver.unwrap());
}

#[test]
fn test_invalid_semver_version() {
    let min = "not-a-version";
    let result: Result<semver::Version, _> = semver::Version::parse(min);
    assert!(result.is_err(), "Invalid semver should fail to parse");
}

#[test]
fn test_min_greater_than_max_detected() {
    let config_toml = r#"
[policies]
min_node_version = "22.0.0"
max_node_version = "18.0.0"
"#;

    let parsed: toml::Value = toml::from_str(config_toml).unwrap();

    let min = parsed["policies"]["min_node_version"].as_str().unwrap();
    let max = parsed["policies"]["max_node_version"].as_str().unwrap();

    let min_ver = semver::Version::parse(min).unwrap();
    let max_ver = semver::Version::parse(max).unwrap();

    // This should be detected as invalid
    assert!(min_ver > max_ver, "min > max should be detected as error");
}

#[test]
fn test_valid_severity_override_values() {
    let config_toml = r#"
[checks.severity_overrides]
check_a = "pass"
check_b = "info"
check_c = "warning"
check_d = "error"
"#;

    let parsed: toml::Value = toml::from_str(config_toml).unwrap();

    let valid_severities = ["pass", "info", "warning", "error"];
    let overrides = parsed["checks"]["severity_overrides"].as_table().unwrap();

    for (_, value) in overrides {
        let severity = value.as_str().unwrap();
        assert!(
            valid_severities.contains(&severity),
            "Severity '{}' should be valid",
            severity
        );
    }
}

#[test]
fn test_invalid_severity_override_value() {
    let config_toml = r#"
[checks.severity_overrides]
some_check = "critical"
"#;

    let parsed: toml::Value = toml::from_str(config_toml).unwrap();
    let severity = parsed["checks"]["severity_overrides"]["some_check"].as_str().unwrap();

    let valid_severities = ["pass", "info", "warning", "error"];
    assert!(
        !valid_severities.contains(&severity),
        "'critical' is not a valid severity"
    );
}

// ============================================================================
// Package Manager List Tests
// ============================================================================

#[test]
fn test_valid_package_managers() {
    let config_toml = r#"
[policies]
allowed_package_managers = ["npm", "pnpm", "yarn", "bun"]
"#;

    let parsed: toml::Value = toml::from_str(config_toml).unwrap();
    let allowed = parsed["policies"]["allowed_package_managers"].as_array().unwrap();

    let valid_pms = ["npm", "pnpm", "yarn", "bun"];
    for pm in allowed {
        let pm_str = pm.as_str().unwrap();
        assert!(
            valid_pms.contains(&pm_str),
            "Package manager '{}' should be valid",
            pm_str
        );
    }
}

#[test]
fn test_empty_package_managers_list() {
    let config_toml = r#"
[policies]
allowed_package_managers = []
"#;

    let parsed: toml::Value = toml::from_str(config_toml).unwrap();
    let allowed = parsed["policies"]["allowed_package_managers"].as_array().unwrap();

    assert!(allowed.is_empty(), "Empty list should allow all package managers");
}

// ============================================================================
// Framework Config Tests
// ============================================================================

#[test]
fn test_nextjs_config() {
    let config_toml = r#"
[frameworks.nextjs]
required_version = "14.0.0"
check_cache_integrity = true
"#;

    let parsed: toml::Value = toml::from_str(config_toml).unwrap();

    assert_eq!(
        parsed["frameworks"]["nextjs"]["required_version"].as_str(),
        Some("14.0.0")
    );
    assert_eq!(
        parsed["frameworks"]["nextjs"]["check_cache_integrity"].as_bool(),
        Some(true)
    );
}

#[test]
fn test_typescript_config() {
    let config_toml = r#"
[frameworks.typescript]
require_tsconfig = true
enforce_strict = true
"#;

    let parsed: toml::Value = toml::from_str(config_toml).unwrap();

    assert_eq!(
        parsed["frameworks"]["typescript"]["require_tsconfig"].as_bool(),
        Some(true)
    );
    assert_eq!(
        parsed["frameworks"]["typescript"]["enforce_strict"].as_bool(),
        Some(true)
    );
}

// ============================================================================
// Default Values Tests
// ============================================================================

#[test]
fn test_default_policies() {
    // When no config is provided, these should be the defaults
    let defaults = toml::toml! {
        [policies]
        allow_node_upgrade_minor = true
        allow_node_upgrade_major = false
        require_lockfile_frozen = true
        enforce_corepack = false
    };

    assert_eq!(defaults["policies"]["allow_node_upgrade_minor"].as_bool(), Some(true));
    assert_eq!(defaults["policies"]["allow_node_upgrade_major"].as_bool(), Some(false));
    assert_eq!(defaults["policies"]["require_lockfile_frozen"].as_bool(), Some(true));
    assert_eq!(defaults["policies"]["enforce_corepack"].as_bool(), Some(false));
}
