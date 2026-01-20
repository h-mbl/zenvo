//! Utility functions for Zenvo
//! This module provides common utilities including command execution with timeout.

use anyhow::Result;
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

/// Default timeout for external commands (30 seconds)
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Short timeout for quick commands (5 seconds)
pub const SHORT_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Long timeout for potentially slow commands (60 seconds)
#[allow(dead_code)]
pub const LONG_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// Result of running a command with timeout
#[derive(Debug)]
pub enum CommandResult {
    /// Command completed successfully with output
    Success(Output),
    /// Command failed with output
    Failed(Output),
    /// Command timed out and was killed
    TimedOut,
    /// Command could not be started
    SpawnError(String),
}

impl CommandResult {
    /// Returns true if the command succeeded
    #[allow(dead_code)]
    pub fn is_success(&self) -> bool {
        matches!(self, CommandResult::Success(_))
    }

    /// Get the output if the command completed (success or failure)
    #[allow(dead_code)]
    pub fn output(&self) -> Option<&Output> {
        match self {
            CommandResult::Success(o) | CommandResult::Failed(o) => Some(o),
            _ => None,
        }
    }

    /// Get stdout as string if available
    #[allow(dead_code)]
    pub fn stdout_string(&self) -> Option<String> {
        self.output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    }

    /// Get stderr as string if available
    #[allow(dead_code)]
    pub fn stderr_string(&self) -> Option<String> {
        self.output()
            .map(|o| String::from_utf8_lossy(&o.stderr).to_string())
    }
}

/// Run a command with a timeout
///
/// # Arguments
/// * `cmd` - The command to run
/// * `args` - Arguments to pass to the command
/// * `timeout` - Maximum time to wait for the command
///
/// # Returns
/// A `CommandResult` indicating success, failure, timeout, or spawn error

pub fn run_command_with_timeout(cmd: &str, args: &[&str], timeout: Duration) -> CommandResult {
    // On Windows, run through cmd.exe to properly find .cmd/.bat files in PATH
    #[cfg(windows)]
    let mut child = {
        let full_cmd = if args.is_empty() {
            cmd.to_string()
        } else {
            format!("{} {}", cmd, args.join(" "))
        };
        match Command::new("cmd")
            .args(["/C", &full_cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return CommandResult::SpawnError(format!("Failed to start '{}': {}", cmd, e)),
        }
    };

    #[cfg(not(windows))]
    let mut child = match Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return CommandResult::SpawnError(format!("Failed to start '{}': {}", cmd, e)),
    };

    // Wait for the process with timeout
    match child.wait_timeout(timeout) {
        Ok(Some(status)) => {
            // Process completed within timeout
            let output = match child.wait_with_output() {
                Ok(o) => o,
                Err(e) => {
                    return CommandResult::SpawnError(format!(
                        "Failed to get output from '{}': {}",
                        cmd, e
                    ))
                }
            };

            if status.success() {
                CommandResult::Success(output)
            } else {
                CommandResult::Failed(output)
            }
        }
        Ok(None) => {
            // Timeout - kill the process
            let _ = child.kill();
            let _ = child.wait(); // Reap the zombie process
            CommandResult::TimedOut
        }
        Err(e) => CommandResult::SpawnError(format!("Failed to wait for '{}': {}", cmd, e)),
    }
}

/// Run a command with timeout and return Result<Output>
///
/// This is a convenience function that converts CommandResult to anyhow::Result
#[allow(dead_code)]
pub fn run_command_timeout_result(
    cmd: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Output> {
    match run_command_with_timeout(cmd, args, timeout) {
        CommandResult::Success(output) => Ok(output),
        CommandResult::Failed(output) => Ok(output), // Return output even on failure
        CommandResult::TimedOut => {
            anyhow::bail!(
                "Command '{}' timed out after {:?}",
                cmd,
                timeout
            )
        }
        CommandResult::SpawnError(e) => {
            anyhow::bail!("{}", e)
        }
    }
}

/// Run a command with default timeout
#[allow(dead_code)]
pub fn run_command(cmd: &str, args: &[&str]) -> CommandResult {
    run_command_with_timeout(cmd, args, DEFAULT_COMMAND_TIMEOUT)
}

/// Check if a command exists and is executable
#[allow(dead_code)]
pub fn command_exists(cmd: &str) -> bool {
    matches!(
        run_command_with_timeout(cmd, &["--version"], SHORT_COMMAND_TIMEOUT),
        CommandResult::Success(_) | CommandResult::Failed(_)
    )
}

/// Run a command and get stdout as string, or None if failed/timed out
#[allow(dead_code)]
pub fn run_command_stdout(cmd: &str, args: &[&str], timeout: Duration) -> Option<String> {
    match run_command_with_timeout(cmd, args, timeout) {
        CommandResult::Success(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if stdout.is_empty() {
                None
            } else {
                Some(stdout)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_command_success() {
        // Use a command that should exist on all platforms
        #[cfg(windows)]
        let result = run_command_with_timeout("cmd", &["/c", "echo", "hello"], SHORT_COMMAND_TIMEOUT);
        #[cfg(not(windows))]
        let result = run_command_with_timeout("echo", &["hello"], SHORT_COMMAND_TIMEOUT);

        assert!(result.is_success());
    }

    #[test]
    fn test_run_command_spawn_error() {
        let result = run_command_with_timeout(
            "nonexistent_command_xyz_123",
            &[],
            SHORT_COMMAND_TIMEOUT,
        );

        matches!(result, CommandResult::SpawnError(_));
    }

    #[test]
    fn test_command_result_stdout() {
        #[cfg(windows)]
        let result = run_command_with_timeout("cmd", &["/c", "echo", "test"], SHORT_COMMAND_TIMEOUT);
        #[cfg(not(windows))]
        let result = run_command_with_timeout("echo", &["test"], SHORT_COMMAND_TIMEOUT);

        let stdout = result.stdout_string().unwrap();
        assert!(stdout.contains("test"));
    }
}
