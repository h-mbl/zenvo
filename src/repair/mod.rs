use anyhow::Result;
use std::process::Command;

use crate::checks::CheckResult;

#[derive(Debug, Clone)]
pub struct RepairAction {
    pub description: String,
    pub command: String,
    pub is_safe: bool,
}

/// Context for generating repair actions
#[derive(Debug, Clone, Default)]
pub struct RepairContext {
    /// Current package manager (npm, yarn, pnpm, bun)
    pub package_manager: String,
    /// Node version manager if detected (volta, fnm, nvm, system)
    pub node_version_manager: Option<String>,
    /// Target Node version from env.lock
    pub target_node_version: Option<String>,
}

impl RepairContext {
    pub fn new(package_manager: &str) -> Self {
        Self {
            package_manager: package_manager.to_string(),
            node_version_manager: None,
            target_node_version: None,
        }
    }

    pub fn with_node_version_manager(mut self, manager: Option<String>) -> Self {
        self.node_version_manager = manager;
        self
    }

    pub fn with_target_node_version(mut self, version: Option<String>) -> Self {
        self.target_node_version = version;
        self
    }

    /// Get the install command for the current package manager
    pub fn install_command(&self) -> &'static str {
        match self.package_manager.as_str() {
            "pnpm" => "pnpm install --frozen-lockfile",
            "yarn" => "yarn install --frozen-lockfile",
            "bun" => "bun install --frozen-lockfile",
            _ => "npm ci",
        }
    }

    /// Get the install command (without frozen lockfile - for generating lockfile)
    pub fn install_command_no_frozen(&self) -> &'static str {
        match self.package_manager.as_str() {
            "pnpm" => "pnpm install",
            "yarn" => "yarn install",
            "bun" => "bun install",
            _ => "npm install",
        }
    }

    /// Get the command to switch Node version
    pub fn node_switch_command(&self, version: &str) -> String {
        match self.node_version_manager.as_deref() {
            Some("volta") => format!("volta pin node@{}", version),
            Some("fnm") => format!("fnm use {}", version),
            Some("nvm") => format!("nvm use {}", version),
            _ => {
                // Default to nvm if no manager detected, but mention alternatives
                format!("nvm use {} (or volta pin node@{} / fnm use {})", version, version, version)
            }
        }
    }

    /// Get commands to clear package manager caches
    pub fn clear_cache_commands(&self) -> Vec<(&'static str, &'static str)> {
        match self.package_manager.as_str() {
            "pnpm" => vec![
                ("Clear pnpm cache", "pnpm store prune"),
            ],
            "yarn" => vec![
                ("Clear yarn cache", "yarn cache clean"),
            ],
            "bun" => vec![
                ("Clear bun cache (manual)", "rm -rf ~/.bun/install/cache"),
            ],
            _ => vec![
                ("Clear npm cache", "npm cache clean --force"),
            ],
        }
    }
}

/// Generate repair plan with context (preferred method)
pub fn generate_repair_plan_with_context(
    issues: &[&CheckResult],
    context: &RepairContext,
) -> Result<Vec<RepairAction>> {
    let mut actions = Vec::new();

    for issue in issues {
        if let Some(action) = issue_to_action_with_context(issue, context) {
            actions.push(action);
        }
    }

    // Sort: safe actions first
    actions.sort_by(|a, b| b.is_safe.cmp(&a.is_safe));

    Ok(actions)
}


/// Context-aware issue to action mapping
fn issue_to_action_with_context(issue: &CheckResult, context: &RepairContext) -> Option<RepairAction> {
    match issue.name.as_str() {
        "Node version match" => {
            // Extract target version from the issue message or use context
            let target_version = extract_target_version(&issue.message)
                .or_else(|| context.target_node_version.clone())
                .unwrap_or_else(|| "<version>".to_string());

            Some(RepairAction {
                description: format!("Switch Node.js to version {}", target_version),
                command: context.node_switch_command(&target_version),
                is_safe: true,
            })
        }

        "Package manager match" => Some(RepairAction {
            description: "Use correct package manager".to_string(),
            command: issue.suggested_fix.clone().unwrap_or_else(|| {
                format!("Use {} instead", context.package_manager)
            }),
            is_safe: true,
        }),

        "node_modules exists" => Some(RepairAction {
            description: format!("Install dependencies using {}", context.package_manager),
            command: context.install_command().to_string(),
            is_safe: true,
        }),

        "node_modules in sync" | "node_modules integrity" => Some(RepairAction {
            description: format!("Reinstall dependencies using {}", context.package_manager),
            command: format!("rm -rf node_modules && {}", context.install_command()),
            is_safe: true,
        }),

        "Lockfile exists" => Some(RepairAction {
            // Need to regenerate lockfile - not safe
            description: format!("Generate lockfile using {}", context.package_manager),
            command: context.install_command_no_frozen().to_string(),
            is_safe: false,
        }),

        "Lockfile integrity" | "Lockfile hash match" => Some(RepairAction {
            description: "Update env.lock to match current lockfile".to_string(),
            command: "zenvo lock".to_string(),
            is_safe: true,
        }),

        "Lockfile corrupted" => {
            // Need to regenerate lockfile - not safe
            Some(RepairAction {
                description: format!("Regenerate corrupted lockfile using {}", context.package_manager),
                command: format!("rm -f {} && {}",
                    get_lockfile_name(&context.package_manager),
                    context.install_command_no_frozen()
                ),
                is_safe: false,
            })
        }

        "Single lockfile" => Some(RepairAction {
            // Requires manual review
            description: "Remove duplicate lockfiles".to_string(),
            command: "Review and remove unused lockfile manually".to_string(),
            is_safe: false,
        }),

        "npm cache integrity" | "Cache corrupted" => {
            let cache_cmds = context.clear_cache_commands();
            if let Some((desc, cmd)) = cache_cmds.first() {
                // Manual commands (like bun) need user review - not safe
                let is_safe = !desc.contains("manual");
                Some(RepairAction {
                    description: desc.to_string(),
                    command: cmd.to_string(),
                    is_safe,
                })
            } else {
                None
            }
        }

        "TypeScript config" => Some(RepairAction {
            description: "Initialize TypeScript config".to_string(),
            command: match context.package_manager.as_str() {
                "pnpm" => "pnpm exec tsc --init".to_string(),
                "yarn" => "yarn tsc --init".to_string(),
                "bun" => "bun x tsc --init".to_string(),
                _ => "npx tsc --init".to_string(),
            },
            is_safe: true,
        }),

        "ESLint config" => Some(RepairAction {
            description: "Initialize ESLint config".to_string(),
            command: match context.package_manager.as_str() {
                "pnpm" => "pnpm create @eslint/config".to_string(),
                "yarn" => "yarn create @eslint/config".to_string(),
                "bun" => "bun create @eslint/config".to_string(),
                _ => "npm init @eslint/config".to_string(),
            },
            is_safe: false,
        }),

        "Corepack available" | "Corepack enabled" => Some(RepairAction {
            description: "Enable Corepack".to_string(),
            command: "corepack enable".to_string(),
            is_safe: true,
        }),

        "Prettier config" => Some(RepairAction {
            description: "Create Prettier config".to_string(),
            command: "echo '{}' > .prettierrc".to_string(),
            is_safe: true,
        }),

        "Peer dependencies" => Some(RepairAction {
            description: "Install missing peer dependencies".to_string(),
            command: match context.package_manager.as_str() {
                "pnpm" => "pnpm install".to_string(),
                "yarn" => "yarn install".to_string(),
                "bun" => "bun install".to_string(),
                _ => "npm install".to_string(),
            },
            is_safe: true,
        }),

        // Package manager not accessible - provide installation instructions
        "npm accessible" | "yarn accessible" | "pnpm accessible" | "bun accessible" => {
            // Requires review as it installs globally - not safe
            let pm = issue.name.replace(" accessible", "");
            Some(RepairAction {
                description: format!("Install {} package manager", pm),
                // Use corepack for yarn/pnpm, or provide manual instructions
                command: match pm.as_str() {
                    "yarn" => "corepack enable && corepack prepare yarn@stable --activate".to_string(),
                    "pnpm" => "corepack enable && corepack prepare pnpm@latest --activate".to_string(),
                    "bun" => "npm install -g bun".to_string(),
                    _ => "npm is included with Node.js - reinstall Node.js".to_string(),
                },
                is_safe: false,
            })
        }

        // Node.js not accessible - install using version manager or system package
        "Node.js accessible" => {
            let target_version = context.target_node_version.as_deref().unwrap_or("--lts");
            let major_version = target_version.split('.').next().unwrap_or("20");

            // Detect version manager via env vars (works even if node isn't installed)
            let cmd = if std::env::var("VOLTA_HOME").is_ok() {
                format!("volta install node@{}", target_version)
            } else if std::env::var("FNM_DIR").is_ok() || std::env::var("FNM_MULTISHELL_PATH").is_ok() {
                format!("fnm install {}", target_version)
            } else if std::env::var("NVM_DIR").is_ok() {
                format!("nvm install {}", target_version)
            } else if cfg!(windows) {
                "winget install OpenJS.NodeJS.LTS".to_string()
            } else if cfg!(target_os = "macos") {
                format!("brew install node@{}", major_version)
            } else {
                // Linux: use NodeSource setup script
                format!(
                    "curl -fsSL https://deb.nodesource.com/setup_{}.x | sudo -E bash - && sudo apt-get install -y nodejs",
                    major_version
                )
            };

            let desc = if target_version == "--lts" {
                "Install Node.js (LTS)".to_string()
            } else {
                format!("Install Node.js {}", target_version)
            };

            Some(RepairAction {
                description: desc,
                command: cmd,
                is_safe: false,
            })
        }

        _ => {
            // For issues without specific repair, suggest manual fix
            if issue.suggested_fix.is_some() {
                Some(RepairAction {
                    description: issue.name.clone(),
                    command: issue.suggested_fix.clone().unwrap(),
                    is_safe: false,
                })
            } else {
                None
            }
        }
    }
}


/// Extract target version from error message like "Expected 20.11.0 but found 18.0.0"
fn extract_target_version(message: &str) -> Option<String> {
    if message.starts_with("Expected ") {
        // Format: "Expected X.Y.Z but found ..."
        let parts: Vec<&str> = message.split_whitespace().collect();
        if parts.len() >= 2 {
            return Some(parts[1].to_string());
        }
    }
    None
}

/// Get the lockfile name for a package manager
fn get_lockfile_name(package_manager: &str) -> &'static str {
    match package_manager {
        "pnpm" => "pnpm-lock.yaml",
        "yarn" => "yarn.lock",
        "bun" => "bun.lockb",
        _ => "package-lock.json",
    }
}

pub fn execute_repair(action: &RepairAction) -> Result<()> {
    // Skip non-executable commands (manual instructions)
    if action.command.contains("manually")
        || action.command.contains("Manual")
        || action.command.contains("reinstall Node.js")
    {
        return Ok(());
    }

    // Execute command through shell to properly resolve PATH and handle operators like &&
    #[cfg(windows)]
    let output = Command::new("cmd")
        .args(["/C", &action.command])
        .output()?;

    #[cfg(not(windows))]
    let output = Command::new("sh")
        .args(["-c", &action.command])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Filter out common warning lines that don't indicate real failures
        let is_only_warnings = stderr.lines().all(|line| {
            line.trim().is_empty()
                || line.starts_with("warning ")
                || line.starts_with("npm WARN")
                || line.contains("deprecated")
        });

        // If stderr only contains warnings and stdout looks successful, don't fail
        if is_only_warnings && !stdout.contains("error") && !stdout.contains("ERR!") {
            return Ok(());
        }

        let error_msg = if stderr.is_empty() { stdout } else { stderr };
        anyhow::bail!("Command failed: {}", error_msg.trim());
    }

    Ok(())
}
