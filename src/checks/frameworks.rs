use anyhow::Result;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::CheckResult;

/// Parsed semantic version
#[derive(Debug, Clone, Default)]
struct ParsedVersion {
    major: u32,
    minor: u32,
    #[allow(dead_code)]
    patch: u32,
}

impl ParsedVersion {
    /// Parse a version string like "20.11.0" or "5.3.2"
    /// Returns None if the version cannot be parsed
    fn parse(version: &str) -> Option<Self> {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.is_empty() {
            return None;
        }

        let major = parts[0].parse::<u32>().ok()?;
        let minor = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);

        Some(Self { major, minor, patch })
    }

    /// Check if this version meets the minimum required
    fn meets_minimum(&self, min_major: u32, min_minor: u32) -> bool {
        self.major > min_major || (self.major == min_major && self.minor >= min_minor)
    }
}

/// Get the current Node.js version
fn get_current_node_version() -> Option<String> {
    let output = Command::new("node").arg("--version").output().ok()?;
    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout)
            .trim()
            .trim_start_matches('v')
            .to_string();
        if version.is_empty() {
            return None;
        }
        Some(version)
    } else {
        None
    }
}

/// Check if the Node.js version meets the minimum required
fn check_node_version_meets_minimum(node_version: &str, min_major: u32, min_minor: u32) -> bool {
    match ParsedVersion::parse(node_version) {
        Some(version) => version.meets_minimum(min_major, min_minor),
        None => {
            // If we can't parse the version, assume it doesn't meet requirements
            // This is safer than assuming it does
            false
        }
    }
}

/// Read the `engines.node` field from a package's package.json in node_modules
fn get_package_engines_node(package_name: &str) -> Option<String> {
    let pkg_path = Path::new("node_modules")
        .join(package_name)
        .join("package.json");

    let content = fs::read_to_string(pkg_path).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;

    pkg.get("engines")?
        .get("node")?
        .as_str()
        .map(|s| s.to_string())
}

/// Parse minimum version from engines constraint like ">=14.17" or ">=18.17.0"
fn parse_min_version_from_constraint(constraint: &str) -> Option<(u32, u32)> {
    // Handle formats: ">=14.17", ">=18.17.0", "^18.17.0", ">=18.17.0 || >=20.0.0"
    let constraint = constraint.trim();

    // Take the first constraint if there are multiple (||)
    let first_constraint = constraint.split("||").next()?.trim();

    // Remove prefix operators
    let version_str = first_constraint
        .trim_start_matches(">=")
        .trim_start_matches(">")
        .trim_start_matches("^")
        .trim_start_matches("~")
        .trim();

    let parsed = ParsedVersion::parse(version_str)?;
    Some((parsed.major, parsed.minor))
}

/// Check package/Node.js compatibility by reading engines from node_modules
fn check_package_node_compatibility(
    package_name: &str,
    package_version: &str,
    node_version: &str,
    check_name: &str,
) -> Option<CheckResult> {
    // Read engines.node from the installed package
    let engines_node = match get_package_engines_node(package_name) {
        Some(engines) => engines,
        None => {
            // No engines field, assume compatible
            return Some(CheckResult::pass(check_name, "frameworks"));
        }
    };

    // Parse the minimum required version
    let (min_major, min_minor) = match parse_min_version_from_constraint(&engines_node) {
        Some(v) => v,
        None => {
            // Can't parse constraint, skip check
            return Some(CheckResult::pass(check_name, "frameworks"));
        }
    };

    // Check if current Node meets the requirement
    if check_node_version_meets_minimum(node_version, min_major, min_minor) {
        Some(CheckResult::pass(check_name, "frameworks"))
    } else {
        Some(
            CheckResult::error(
                check_name,
                "frameworks",
                &format!(
                    "{} {} requires Node.js {}, but found {}",
                    package_name, package_version, engines_node, node_version
                ),
            )
            .with_fix(&format!("Upgrade Node.js to version {}.{}+", min_major, min_minor)),
        )
    }
}

pub fn run_checks() -> Result<Vec<CheckResult>> {
    let mut results = Vec::new();

    // Read package.json
    let pkg_json = match fs::read_to_string("package.json") {
        Ok(content) => content,
        Err(_) => return Ok(results),
    };

    let pkg: serde_json::Value = match serde_json::from_str(&pkg_json) {
        Ok(v) => v,
        Err(_) => return Ok(results),
    };

    let deps = pkg.get("dependencies").and_then(|d| d.as_object());
    let dev_deps = pkg.get("devDependencies").and_then(|d| d.as_object());

    let get_version = |name: &str| -> Option<String> {
        deps.and_then(|d| d.get(name))
            .or_else(|| dev_deps.and_then(|d| d.get(name)))
            .and_then(|v| v.as_str())
            .map(|s| s.trim_start_matches('^').trim_start_matches('~').to_string())
    };

    // Check 1: React/ReactDOM version match
    if let (Some(react), Some(react_dom)) = (get_version("react"), get_version("react-dom")) {
        let react_major = react.split('.').next().unwrap_or("");
        let react_dom_major = react_dom.split('.').next().unwrap_or("");

        if react_major != react_dom_major {
            results.push(
                CheckResult::error(
                    "React/ReactDOM match",
                    "frameworks",
                    &format!(
                        "react@{} and react-dom@{} major versions don't match",
                        react, react_dom
                    ),
                )
                .with_fix("Ensure react and react-dom have the same major version"),
            );
        } else {
            results.push(CheckResult::pass("React/ReactDOM match", "frameworks"));
        }
    }

    // Check 2: Next.js + Node.js compatibility (reads engines from node_modules)
    if let Some(next_version) = get_version("next") {
        if let Some(node_version) = get_current_node_version() {
            if let Some(result) = check_package_node_compatibility(
                "next",
                &next_version,
                &node_version,
                "Next.js/Node compatibility",
            ) {
                results.push(result);
            }
        } else {
            results.push(
                CheckResult::warning(
                    "Next.js/Node compatibility",
                    "frameworks",
                    "Could not detect Node.js version to verify Next.js compatibility",
                )
            );
        }

        // Check for .next cache integrity
        check_nextjs_cache(&mut results);
    }

    // Check 3: TypeScript config and compatibility
    if let Some(ts_version) = get_version("typescript") {
        // Check for tsconfig.json
        if !Path::new("tsconfig.json").exists() {
            results.push(
                CheckResult::warning(
                    "TypeScript config",
                    "frameworks",
                    "TypeScript is installed but tsconfig.json not found",
                )
                .with_fix("Run `npx tsc --init` to create tsconfig.json"),
            );
        } else {
            results.push(CheckResult::pass("TypeScript config", "frameworks"));
        }

        // Check TypeScript/Node.js version compatibility (reads engines from node_modules)
        if let Some(node_version) = get_current_node_version() {
            if let Some(result) = check_package_node_compatibility(
                "typescript",
                &ts_version,
                &node_version,
                "TypeScript/Node compatibility",
            ) {
                results.push(result);
            }
        }
    }

    // Check 4: ESLint config
    if get_version("eslint").is_some() {
        let eslint_configs = [
            ".eslintrc",
            ".eslintrc.js",
            ".eslintrc.json",
            ".eslintrc.yml",
            "eslint.config.js",
        ];

        let has_config = eslint_configs.iter().any(|f| Path::new(f).exists());

        // Also check package.json for eslintConfig
        let has_pkg_config = pkg.get("eslintConfig").is_some();

        if !has_config && !has_pkg_config {
            results.push(
                CheckResult::warning(
                    "ESLint config",
                    "frameworks",
                    "ESLint is installed but no config found",
                )
                .with_fix("Run `npx eslint --init` to create config"),
            );
        } else {
            results.push(CheckResult::pass("ESLint config", "frameworks"));
        }
    }

    // Check 5: Prettier config
    if get_version("prettier").is_some() {
        let prettier_configs = [
            ".prettierrc",
            ".prettierrc.js",
            ".prettierrc.cjs",
            ".prettierrc.mjs",
            ".prettierrc.json",
            ".prettierrc.yml",
            ".prettierrc.yaml",
            ".prettierrc.toml",
            "prettier.config.js",
            "prettier.config.cjs",
            "prettier.config.mjs",
        ];

        let has_config = prettier_configs.iter().any(|f| Path::new(f).exists());

        // Also check package.json for prettier config
        let has_pkg_config = pkg.get("prettier").is_some();

        if !has_config && !has_pkg_config {
            results.push(
                CheckResult::warning(
                    "Prettier config",
                    "frameworks",
                    "Prettier is installed but no config found",
                )
                .with_fix("Create a .prettierrc file with your formatting preferences"),
            );
        } else {
            results.push(CheckResult::pass("Prettier config", "frameworks"));
        }
    }

    // Check 6: Build cache integrity
    check_build_cache_integrity(&mut results);

    Ok(results)
}

/// Check Next.js cache integrity
fn check_nextjs_cache(results: &mut Vec<CheckResult>) {
    let next_dir = Path::new(".next");

    if !next_dir.exists() {
        // No cache is fine - it will be created on build
        return;
    }

    // Check for build manifest (indicates successful build)
    let build_manifest = next_dir.join("build-manifest.json");
    let cache_dir = next_dir.join("cache");

    if build_manifest.exists() {
        // Check if build manifest is valid JSON
        match fs::read_to_string(&build_manifest) {
            Ok(content) => {
                if serde_json::from_str::<serde_json::Value>(&content).is_ok() {
                    results.push(CheckResult::pass("Next.js cache valid", "frameworks"));
                } else {
                    results.push(
                        CheckResult::warning(
                            "Next.js cache corrupted",
                            "frameworks",
                            "build-manifest.json is corrupted",
                        )
                        .with_fix("Run `rm -rf .next && npm run build` to rebuild"),
                    );
                }
            }
            Err(_) => {
                results.push(
                    CheckResult::warning(
                        "Next.js cache unreadable",
                        "frameworks",
                        "Cannot read build-manifest.json",
                    )
                    .with_fix("Run `rm -rf .next && npm run build` to rebuild"),
                );
            }
        }
    } else if cache_dir.exists() {
        // Cache exists but no build manifest - partial/incomplete build
        results.push(
            CheckResult::warning(
                "Next.js cache incomplete",
                "frameworks",
                ".next cache exists but no build manifest found",
            )
            .with_fix("Run `npm run build` to complete the build"),
        );
    }
}

/// Check integrity of common build caches
fn check_build_cache_integrity(results: &mut Vec<CheckResult>) {
    let caches = [
        (".turbo", "Turbo cache"),
        (".vite", "Vite cache"),
        ("dist", "Build output"),
        ("build", "Build output"),
    ];

    for (cache_path, cache_name) in caches {
        let path = Path::new(cache_path);
        if path.exists() && path.is_dir() {
            // Check if the directory is empty or has issues
            match fs::read_dir(path) {
                Ok(entries) => {
                    let entry_count = entries.count();
                    if entry_count == 0 {
                        results.push(
                            CheckResult::warning(
                                &format!("{} empty", cache_name),
                                "frameworks",
                                &format!("{} directory exists but is empty", cache_path),
                            )
                            .with_fix(&format!("Remove empty {} or rebuild", cache_path)),
                        );
                    } else {
                        results.push(CheckResult::pass(
                            &format!("{} exists", cache_name),
                            "frameworks",
                        ));
                    }
                }
                Err(_) => {
                    results.push(
                        CheckResult::warning(
                            &format!("{} unreadable", cache_name),
                            "frameworks",
                            &format!("Cannot read {} directory", cache_path),
                        )
                        .with_fix(&format!("Check permissions on {}", cache_path)),
                    );
                }
            }
        }
    }
}
