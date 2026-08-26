use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::fs_tools;
use crate::AppState;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 900;
const MAX_OUTPUT_BYTES: usize = 200_000;

/// Runs a shell command with its working directory pinned inside
/// base_dir (via the same path-escape guard fs_tools uses), under a
/// hard timeout so a hung test or build can't hang the whole server.
/// Intended for running tests, linters, builds, and read-only
/// verification commands (e.g. `curl` to confirm a live endpoint) —
/// not as a substitute for the dedicated fs_*/git_* tools when those
/// already cover the operation.
pub async fn exec_command(
    state: &AppState,
    repo_path: &str,
    command: &str,
    timeout_secs: Option<u64>,
) -> Result<String, String> {
    let cwd = fs_tools::resolve(state, repo_path)?;
    if !cwd.is_dir() {
        return Err(format!("{repo_path} is not a directory"));
    }

    let secs = timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS).min(MAX_TIMEOUT_SECS);

    let child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn command: {e}"))?;

    let output = match timeout(Duration::from_secs(secs), child.wait_with_output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(format!("command execution error: {e}")),
        Err(_) => {
            return Err(format!(
                "command timed out after {secs}s — command was still running and was not left \
                 to complete; if this is expected to take longer, pass a higher timeout_secs \
                 (max {MAX_TIMEOUT_SECS}), or if it hung, the command itself is the problem, not \
                 the timeout"
            ))
        }
    };

    let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if stdout.len() > MAX_OUTPUT_BYTES {
        stdout.truncate(MAX_OUTPUT_BYTES);
        stdout.push_str("\n... [stdout truncated]");
    }
    if stderr.len() > MAX_OUTPUT_BYTES {
        stderr.truncate(MAX_OUTPUT_BYTES);
        stderr.push_str("\n... [stderr truncated]");
    }

    let exit_code = output.status.code().unwrap_or(-1);

    Ok(format!(
        "exit_code: {exit_code}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    ))
}
