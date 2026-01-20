use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use super::CheckResult;
use crate::utils::{run_command_with_timeout, CommandResult, DEFAULT_COMMAND_TIMEOUT};

/// Maximum depth for source directory scanning
const MAX_SOURCE_SCAN_DEPTH: usize = 10;

/// Get the installed version of a package from node_modules
fn get_installed_version(package_name: &str) -> Option<String> {
    let pkg_json_path = Path::new("node_modules").join(package_name).join("package.json");
    let content = fs::read_to_string(pkg_json_path).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;
    pkg.get("version").and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Get expected versions from lockfile
fn get_lockfile_versions() -> Result<HashMap<String, String>> {
    let mut versions = HashMap::new();

    // Try npm lockfile first
    if Path::new("package-lock.json").exists() {
        let content = fs::read_to_string("package-lock.json")?;
        let lockfile: serde_json::Value = serde_json::from_str(&content)?;

        // npm lockfile v2/v3 uses "packages" object
        if let Some(packages) = lockfile.get("packages").and_then(|v| v.as_object()) {
            for (key, value) in packages {
                if key.starts_with("node_modules/") && !key.contains("/node_modules/") {
                    let pkg_name = key.strip_prefix("node_modules/").unwrap();
                    // Handle scoped packages
                    let name = if pkg_name.starts_with('@') {
                        let parts: Vec<&str> = pkg_name.splitn(3, '/').collect();
                        if parts.len() >= 2 {
                            format!("{}/{}", parts[0], parts[1])
                        } else {
                            continue;
                        }
                    } else {
                        pkg_name.split('/').next().unwrap_or(pkg_name).to_string()
                    };

                    if let Some(version) = value.get("version").and_then(|v| v.as_str()) {
                        versions.insert(name, version.to_string());
                    }
                }
            }
        }
        // Fallback to dependencies object for older lockfile versions
        else if let Some(dependencies) = lockfile.get("dependencies").and_then(|v| v.as_object()) {
            for (name, value) in dependencies {
                if let Some(version) = value.get("version").and_then(|v| v.as_str()) {
                    versions.insert(name.clone(), version.to_string());
                }
            }
        }
        return Ok(versions);
    }

    // Try pnpm lockfile
    if Path::new("pnpm-lock.yaml").exists() {
        let content = fs::read_to_string("pnpm-lock.yaml")?;
        let lockfile: serde_yaml::Value = serde_yaml::from_str(&content)?;

        // pnpm uses "packages" mapping
        if let Some(packages) = lockfile.get("packages").and_then(|v| v.as_mapping()) {
            for (key, value) in packages {
                if let Some(key_str) = key.as_str() {
                    let pkg_ref = key_str.trim_start_matches('/');
                    let (name, version) = if pkg_ref.starts_with('@') {
                        // Scoped package: @scope/name@version
                        let parts: Vec<&str> = pkg_ref.splitn(3, '/').collect();
                        if parts.len() >= 2 {
                            let name_with_version = parts[1];
                            if let Some(at_idx) = name_with_version.rfind('@') {
                                let name = format!("{}/{}", parts[0], &name_with_version[..at_idx]);
                                let ver = &name_with_version[at_idx + 1..];
                                (name, ver.to_string())
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    } else {
                        // Regular package: name@version
                        if let Some(at_idx) = pkg_ref.rfind('@') {
                            let name = &pkg_ref[..at_idx];
                            let ver = &pkg_ref[at_idx + 1..];
                            (name.to_string(), ver.to_string())
                        } else {
                            continue;
                        }
                    };

                    // Also check for version field in the value object
                    let final_version = value
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or(version);

                    if !name.is_empty() {
                        versions.insert(name, final_version);
                    }
                }
            }
        }
    }

    Ok(versions)
}

/// Check if node_modules matches lockfile versions
fn check_node_modules_match(results: &mut Vec<CheckResult>) -> Result<()> {
    // Skip if node_modules doesn't exist
    if !Path::new("node_modules").exists() {
        return Ok(());
    }

    let lockfile_versions = match get_lockfile_versions() {
        Ok(v) => v,
        Err(_) => return Ok(()), // Can't read lockfile, skip check
    };

    if lockfile_versions.is_empty() {
        return Ok(());
    }

    // Get direct dependencies from package.json
    let pkg_json = match fs::read_to_string("package.json") {
        Ok(content) => content,
        Err(_) => return Ok(()),
    };

    let pkg: serde_json::Value = match serde_json::from_str(&pkg_json) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let mut direct_deps: HashSet<String> = HashSet::new();
    for field in ["dependencies", "devDependencies"] {
        if let Some(deps) = pkg.get(field).and_then(|v| v.as_object()) {
            for key in deps.keys() {
                direct_deps.insert(key.clone());
            }
        }
    }

    // Check only direct dependencies
    let mut mismatches: Vec<String> = Vec::new();
    for dep in &direct_deps {
        if let Some(expected_version) = lockfile_versions.get(dep) {
            if let Some(installed_version) = get_installed_version(dep) {
                if &installed_version != expected_version {
                    mismatches.push(format!(
                        "{}: expected {} but found {}",
                        dep, expected_version, installed_version
                    ));
                }
            }
        }
    }

    if mismatches.is_empty() {
        results.push(CheckResult::pass("node_modules matches lockfile", "deps"));
    } else {
        let msg = if mismatches.len() <= 2 {
            format!("Version mismatches: {}", mismatches.join("; "))
        } else {
            format!(
                "Version mismatches: {}; and {} more",
                mismatches[..2].join("; "),
                mismatches.len() - 2
            )
        };
        results.push(
            CheckResult::error("node_modules matches lockfile", "deps", &msg)
                .with_fix("Run `npm ci` or `pnpm install --frozen-lockfile` to reinstall")
        );
    }

    Ok(())
}

pub fn run_checks() -> Result<Vec<CheckResult>> {
    let mut results = Vec::new();

    // Only run if node_modules exists
    if !Path::new("node_modules").exists() {
        return Ok(results);
    }

    // Check 1: node_modules matches lockfile versions
    check_node_modules_match(&mut results)?;

    // Check 2: .bin directory exists
    if Path::new("node_modules/.bin").exists() {
        results.push(CheckResult::pass("Binaries installed", "deps"));
    }

    // Check 3: Deprecated packages
    if let Ok(pkg_json) = fs::read_to_string("package.json") {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&pkg_json) {
            check_deprecated_packages(&pkg, &mut results);
        }
    }

    // Check 4: Peer dependency issues
    check_peer_dependencies(&mut results)?;

    // Check 5: Phantom dependencies
    check_phantom_dependencies(&mut results)?;

    Ok(results)
}

/// Check for known deprecated packages
fn check_deprecated_packages(pkg: &serde_json::Value, results: &mut Vec<CheckResult>) {
    let deprecated = [
        ("request", "Use `node-fetch` or `axios` instead"),
        ("node-sass", "Use `sass` (Dart Sass) instead"),
        ("tslint", "Use `eslint` with `@typescript-eslint` instead"),
        ("left-pad", "Use String.prototype.padStart() instead"),
        ("moment", "Consider `date-fns` or `dayjs` for smaller bundle size"),
    ];

    let deps = pkg.get("dependencies").and_then(|d| d.as_object());
    let dev_deps = pkg.get("devDependencies").and_then(|d| d.as_object());

    for (dep_name, suggestion) in deprecated {
        let in_deps = deps.map(|d| d.contains_key(dep_name)).unwrap_or(false);
        let in_dev = dev_deps.map(|d| d.contains_key(dep_name)).unwrap_or(false);

        if in_deps || in_dev {
            results.push(
                CheckResult::warning(
                    &format!("Deprecated: {}", dep_name),
                    "deps",
                    &format!("`{}` is deprecated or has better alternatives", dep_name),
                )
                .with_fix(suggestion),
            );
        }
    }
}

/// Check peer dependency issues using npm ls
fn check_peer_dependencies(results: &mut Vec<CheckResult>) -> Result<()> {
    // Try to run npm ls --json to get dependency tree with timeout
    let cmd_result = run_command_with_timeout("npm", &["ls", "--json", "--depth=1"], DEFAULT_COMMAND_TIMEOUT);

    match cmd_result {
        CommandResult::Success(out) | CommandResult::Failed(out) => {
            // npm ls may return non-zero exit code when there are issues, but still produce valid JSON
            if let Ok(json_str) = String::from_utf8(out.stdout) {
                if let Ok(tree) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    // Check for problems in the dependency tree
                    if let Some(problems) = tree.get("problems").and_then(|p| p.as_array()) {
                        let peer_issues: Vec<_> = problems
                            .iter()
                            .filter_map(|p| p.as_str())
                            .filter(|p| p.contains("peer dep") || p.contains("ERESOLVE"))
                            .collect();

                        if !peer_issues.is_empty() {
                            for issue in peer_issues.iter().take(3) {
                                // Limit to first 3 issues
                                results.push(
                                    CheckResult::warning("Peer dependency conflict", "deps", issue)
                                        .with_fix(
                                            "Run `npm install` to attempt resolution or check versions",
                                        ),
                                );
                            }

                            if peer_issues.len() > 3 {
                                results.push(CheckResult::warning(
                                    "Peer dependencies",
                                    "deps",
                                    &format!(
                                        "{} more peer dependency issues found",
                                        peer_issues.len() - 3
                                    ),
                                ));
                            }
                        } else {
                            results.push(CheckResult::pass("Peer dependencies", "deps"));
                        }
                    } else {
                        results.push(CheckResult::pass("Peer dependencies", "deps"));
                    }
                } else {
                    // Could not parse output, assume OK
                    results.push(CheckResult::pass("Peer dependencies", "deps"));
                }
            } else {
                results.push(CheckResult::pass("Peer dependencies", "deps"));
            }
        }
        CommandResult::TimedOut => {
            results.push(
                CheckResult::warning(
                    "Peer dependencies",
                    "deps",
                    "npm ls command timed out - skipping peer dependency check",
                )
                .with_fix("Try running `npm ls` manually to check for issues"),
            );
        }
        CommandResult::SpawnError(_) => {
            // npm not available, skip this check
            results.push(CheckResult::pass("Peer dependencies", "deps"));
        }
    }

    Ok(())
}

/// Check for phantom dependencies (imports without package.json entry)
fn check_phantom_dependencies(results: &mut Vec<CheckResult>) -> Result<()> {
    // Get declared dependencies
    let pkg_json = match fs::read_to_string("package.json") {
        Ok(content) => content,
        Err(_) => return Ok(()),
    };

    let pkg: serde_json::Value = match serde_json::from_str(&pkg_json) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let mut declared_deps: HashSet<String> = HashSet::new();

    // Collect all declared dependencies
    if let Some(deps) = pkg.get("dependencies").and_then(|d| d.as_object()) {
        for key in deps.keys() {
            declared_deps.insert(key.clone());
        }
    }
    if let Some(deps) = pkg.get("devDependencies").and_then(|d| d.as_object()) {
        for key in deps.keys() {
            declared_deps.insert(key.clone());
        }
    }
    if let Some(deps) = pkg.get("peerDependencies").and_then(|d| d.as_object()) {
        for key in deps.keys() {
            declared_deps.insert(key.clone());
        }
    }
    if let Some(deps) = pkg.get("optionalDependencies").and_then(|d| d.as_object()) {
        for key in deps.keys() {
            declared_deps.insert(key.clone());
        }
    }

    // Add Node.js built-in modules
    let builtins: HashSet<&str> = [
        "assert",
        "buffer",
        "child_process",
        "cluster",
        "console",
        "constants",
        "crypto",
        "dgram",
        "dns",
        "domain",
        "events",
        "fs",
        "http",
        "https",
        "module",
        "net",
        "os",
        "path",
        "perf_hooks",
        "process",
        "punycode",
        "querystring",
        "readline",
        "repl",
        "stream",
        "string_decoder",
        "sys",
        "timers",
        "tls",
        "tty",
        "url",
        "util",
        "v8",
        "vm",
        "wasi",
        "worker_threads",
        "zlib",
    ]
    .into_iter()
    .collect();

    // Scan source files for imports
    let mut phantom_deps: HashSet<String> = HashSet::new();
    let source_dirs = ["src", "lib", "app", "pages", "components"];

    for dir in source_dirs {
        if Path::new(dir).exists() {
            scan_directory_for_imports(Path::new(dir), &declared_deps, &builtins, &mut phantom_deps)?;
        }
    }

    // Also check root level files
    for entry in fs::read_dir(".")? {
        if let Ok(entry) = entry {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "js" || ext == "ts" || ext == "jsx" || ext == "tsx" || ext == "mjs" {
                    if let Some(filename) = path.file_name() {
                        // Skip config files
                        let name = filename.to_string_lossy();
                        if !name.contains("config") && !name.starts_with('.') {
                            scan_file_for_imports(&path, &declared_deps, &builtins, &mut phantom_deps)?;
                        }
                    }
                }
            }
        }
    }

    if phantom_deps.is_empty() {
        results.push(CheckResult::pass("No phantom dependencies", "deps"));
    } else {
        let phantom_list: Vec<_> = phantom_deps.iter().take(5).collect();
        let message = if phantom_deps.len() > 5 {
            format!(
                "Found {} phantom dependencies: {}, and {} more",
                phantom_deps.len(),
                phantom_list
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                phantom_deps.len() - 5
            )
        } else {
            format!(
                "Found phantom dependencies: {}",
                phantom_list
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        results.push(
            CheckResult::warning("Phantom dependencies", "deps", &message)
                .with_fix("Add missing dependencies to package.json or remove unused imports"),
        );
    }

    Ok(())
}

/// Scan a directory for import statements using walkdir for better performance
fn scan_directory_for_imports(
    dir: &Path,
    declared: &HashSet<String>,
    builtins: &HashSet<&str>,
    phantoms: &mut HashSet<String>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    // Use walkdir with max_depth for controlled traversal
    for entry in walkdir::WalkDir::new(dir)
        .max_depth(MAX_SOURCE_SCAN_DEPTH)
        .into_iter()
        .filter_entry(|e| {
            // Skip node_modules and hidden directories
            e.file_name()
                .to_str()
                .map(|s| s != "node_modules" && !s.starts_with('.'))
                .unwrap_or(false)
                || e.depth() == 0 // Always include the root
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "js" || ext == "ts" || ext == "jsx" || ext == "tsx" || ext == "mjs" {
                    scan_file_for_imports(path, declared, builtins, phantoms)?;
                }
            }
        }
    }

    Ok(())
}

/// Scan a single file for import/require statements
fn scan_file_for_imports(
    file: &Path,
    declared: &HashSet<String>,
    builtins: &HashSet<&str>,
    phantoms: &mut HashSet<String>,
) -> Result<()> {
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    for line in content.lines() {
        let line = line.trim();

        // ES6 import
        if line.starts_with("import ") || line.contains(" from ") {
            if let Some(pkg) = extract_package_from_import(line) {
                check_package(&pkg, declared, builtins, phantoms);
            }
        }

        // CommonJS require
        if line.contains("require(") {
            for pkg in extract_packages_from_require(line) {
                check_package(&pkg, declared, builtins, phantoms);
            }
        }

        // Dynamic import
        if line.contains("import(") {
            if let Some(pkg) = extract_package_from_dynamic_import(line) {
                check_package(&pkg, declared, builtins, phantoms);
            }
        }
    }

    Ok(())
}

fn extract_package_from_import(line: &str) -> Option<String> {
    // Find the quoted string after 'from'
    let from_idx = line.find(" from ")?;
    let after_from = &line[from_idx + 6..];

    extract_quoted_string(after_from)
}

fn extract_packages_from_require(line: &str) -> Vec<String> {
    let mut packages = Vec::new();
    let mut search_start = 0;

    while let Some(require_idx) = line[search_start..].find("require(") {
        let start = search_start + require_idx + 8;
        if let Some(pkg) = extract_quoted_string(&line[start..]) {
            packages.push(pkg);
        }
        search_start = start;
    }

    packages
}

fn extract_package_from_dynamic_import(line: &str) -> Option<String> {
    let import_idx = line.find("import(")?;
    let after_import = &line[import_idx + 7..];

    extract_quoted_string(after_import)
}

fn extract_quoted_string(s: &str) -> Option<String> {
    let s = s.trim();

    let (quote_char, start_idx) = if s.starts_with('"') {
        ('"', 1)
    } else if s.starts_with('\'') {
        ('\'', 1)
    } else if s.starts_with('`') {
        ('`', 1)
    } else {
        return None;
    };

    let end_idx = s[start_idx..].find(quote_char)?;
    Some(s[start_idx..start_idx + end_idx].to_string())
}

fn check_package(
    import_path: &str,
    declared: &HashSet<String>,
    builtins: &HashSet<&str>,
    phantoms: &mut HashSet<String>,
) {
    // Skip relative imports
    if import_path.starts_with('.') || import_path.starts_with('/') {
        return;
    }

    // Skip node: protocol
    if import_path.starts_with("node:") {
        return;
    }

    // Extract package name (handle scoped packages)
    let package_name = if import_path.starts_with('@') {
        // Scoped package: @scope/package or @scope/package/subpath
        let parts: Vec<&str> = import_path.splitn(3, '/').collect();
        if parts.len() >= 2 {
            format!("{}/{}", parts[0], parts[1])
        } else {
            import_path.to_string()
        }
    } else {
        // Regular package: package or package/subpath
        import_path
            .split('/')
            .next()
            .unwrap_or(import_path)
            .to_string()
    };

    // Skip built-in modules
    if builtins.contains(package_name.as_str()) {
        return;
    }

    // Check if declared
    if !declared.contains(&package_name) {
        phantoms.insert(package_name);
    }
}
