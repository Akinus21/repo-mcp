use std::process::Stdio;

use tokio::process::Command;

use crate::fs_tools::resolve;
use crate::AppState;

async fn run_git(
    state: &AppState,
    cwd: Option<&std::path::Path>,
    args: &[&str],
) -> Result<String, String> {
    let mut cmd = Command::new("git");

    // Storagebox/network-mounted repo dirs often report ownership metadata
    // that doesn't match this container's effective UID, which trips git's
    // "dubious ownership" safety check. Trust every path under our own
    // sandboxed base_dir on every invocation rather than depending on a
    // persisted global gitconfig entry that a rebuild/restart could lose.
    cmd.args(["-c", "safe.directory=*"]);

    // For every configured host, transparently rewrite plain https://<host>/
    // URLs to include credentials. Callers only ever handle bare,
    // credential-free URLs regardless of which host (Forgejo, GitHub, etc.)
    // they're targeting — the token never appears in a clone/push argument
    // or anything an agent might echo back.
    for cred in &state.git_credentials {
        let plain = format!("https://{}/", cred.host);
        let authenticated = format!("https://{}:{}@{}/", cred.username, cred.token, cred.host);
        cmd.args(["-c", &format!("url.{authenticated}.insteadOf={plain}")]);
    }

    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    // Ensure a sane author identity is always available for commits, without
    // requiring a global gitconfig on the host/container.
    cmd.env("GIT_AUTHOR_NAME", &state.git_author_name)
        .env("GIT_AUTHOR_EMAIL", &state.git_author_email)
        .env("GIT_COMMITTER_NAME", &state.git_author_name)
        .env("GIT_COMMITTER_EMAIL", &state.git_author_email)
        .env("GIT_TERMINAL_PROMPT", "0");

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to spawn git: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(if stdout.trim().is_empty() {
            if stderr.trim().is_empty() {
                "ok".to_string()
            } else {
                stderr
            }
        } else {
            stdout
        })
    } else {
        Err(format!(
            "git {} failed (exit {:?}):\n{}{}",
            args.join(" "),
            output.status.code(),
            stdout,
            stderr
        ))
    }
}

/// Resolve a repo path under base_dir. Unlike fs_tools::resolve, this does
/// not require the target to already exist (needed for `clone`).
fn resolve_repo_dir(state: &AppState, repo_path: &str) -> Result<std::path::PathBuf, String> {
    let base = state
        .base_dir
        .canonicalize()
        .map_err(|e| format!("base_dir error: {e}"))?;
    let joined = if std::path::Path::new(repo_path).is_absolute() {
        std::path::PathBuf::from(repo_path)
    } else {
        base.join(repo_path)
    };
    // Best-effort containment check against the un-canonicalized join;
    // full canonicalization happens after clone/init creates the dir.
    if !joined.starts_with(&base) {
        return Err(format!("path escapes allowed base directory: {repo_path}"));
    }
    Ok(joined)
}

pub async fn clone(
    state: &AppState,
    url: &str,
    dest: &str,
    branch: Option<&str>,
) -> Result<String, String> {
    let dest_path = resolve_repo_dir(state, dest)?;
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let dest_str = dest_path.to_string_lossy().to_string();
    let mut args = vec!["clone", url, &dest_str];
    if let Some(b) = branch {
        args.push("--branch");
        args.push(b);
    }
    run_git(state, None, &args).await
}

pub async fn status(state: &AppState, repo_path: &str) -> Result<String, String> {
    let dir = resolve(state, repo_path)?;
    run_git(state, Some(&dir), &["status", "--short", "--branch"]).await
}

pub async fn add(state: &AppState, repo_path: &str, files: &[String]) -> Result<String, String> {
    let dir = resolve(state, repo_path)?;
    let mut args = vec!["add"];
    if files.is_empty() {
        args.push(".");
    } else {
        args.extend(files.iter().map(|s| s.as_str()));
    }
    run_git(state, Some(&dir), &args).await
}

pub async fn commit(state: &AppState, repo_path: &str, message: &str) -> Result<String, String> {
    let dir = resolve(state, repo_path)?;
    run_git(state, Some(&dir), &["commit", "-m", message]).await
}

pub async fn push(
    state: &AppState,
    repo_path: &str,
    remote: Option<&str>,
    branch: Option<&str>,
) -> Result<String, String> {
    let dir = resolve(state, repo_path)?;
    let mut args = vec!["push"];
    let remote = remote.unwrap_or("origin");
    args.push(remote);
    if let Some(b) = branch {
        args.push(b);
    }
    run_git(state, Some(&dir), &args).await
}

pub async fn pull(
    state: &AppState,
    repo_path: &str,
    remote: Option<&str>,
    branch: Option<&str>,
) -> Result<String, String> {
    let dir = resolve(state, repo_path)?;
    let mut args = vec!["pull"];
    let remote = remote.unwrap_or("origin");
    args.push(remote);
    if let Some(b) = branch {
        args.push(b);
    }
    run_git(state, Some(&dir), &args).await
}

pub async fn branch_list(state: &AppState, repo_path: &str) -> Result<String, String> {
    let dir = resolve(state, repo_path)?;
    run_git(state, Some(&dir), &["branch", "-a"]).await
}

pub async fn checkout(
    state: &AppState,
    repo_path: &str,
    branch: &str,
    create: bool,
) -> Result<String, String> {
    let dir = resolve(state, repo_path)?;
    let mut args = vec!["checkout"];
    if create {
        args.push("-b");
    }
    args.push(branch);
    run_git(state, Some(&dir), &args).await
}

pub async fn diff(state: &AppState, repo_path: &str, staged: bool) -> Result<String, String> {
    let dir = resolve(state, repo_path)?;
    let mut args = vec!["diff"];
    if staged {
        args.push("--staged");
    }
    run_git(state, Some(&dir), &args).await
}

pub async fn log(state: &AppState, repo_path: &str, limit: u32) -> Result<String, String> {
    let dir = resolve(state, repo_path)?;
    let limit_str = format!("-{limit}");
    run_git(
        state,
        Some(&dir),
        &["log", &limit_str, "--oneline", "--decorate"],
    )
    .await
}
