use serde_json::{json, Value};

use crate::{exec_tools, fs_tools, git_tools, AppState};

fn arg_u64_opt(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(|v| v.as_u64())
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing required string argument: {key}"))
}

fn arg_str_opt<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn arg_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn arg_u32(args: &Value, key: &str, default: u32) -> u32 {
    args.get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(default)
}

fn arg_str_array(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub async fn dispatch(state: &AppState, name: &str, args: &Value) -> Result<String, String> {
    match name {
        // --- filesystem ---
        "fs_read_file" => fs_tools::read_file(state, arg_str(args, "path")?),
        "fs_write_file" => fs_tools::write_file(
            state,
            arg_str(args, "path")?,
            arg_str(args, "content")?,
        ),
        "fs_list_directory" => fs_tools::list_directory(state, arg_str(args, "path")?),
        "fs_create_directory" => fs_tools::create_directory(state, arg_str(args, "path")?),
        "fs_delete_path" => fs_tools::delete_path(state, arg_str(args, "path")?),
        "fs_move_path" => {
            fs_tools::move_path(state, arg_str(args, "source")?, arg_str(args, "destination")?)
        }
        "fs_search_files" => fs_tools::search_files(
            state,
            arg_str(args, "path")?,
            arg_str(args, "pattern")?,
        ),
        "edit_file" => fs_tools::edit_file(
            state,
            arg_str(args, "path")?,
            arg_str(args, "old_str")?,
            arg_str(args, "new_str")?,
        ),
        "grep_content" => fs_tools::grep_content(
            state,
            arg_str(args, "path")?,
            arg_str(args, "pattern")?,
        ),

        // --- exec ---
        "exec_command" => {
            if !state.exec_enabled {
                return Err(
                    "exec_command is disabled on this server (REPO_MCP_EXEC_ENABLED is not set to true) — \
                     ask the operator to enable it if running tests/builds/lints is genuinely needed for this task"
                        .to_string(),
                );
            }
            exec_tools::exec_command(
                state,
                arg_str(args, "repo_path")?,
                arg_str(args, "command")?,
                arg_u64_opt(args, "timeout_secs"),
            )
            .await
        }

        // --- git ---
        "git_clone" => {
            git_tools::clone(
                state,
                arg_str(args, "url")?,
                arg_str(args, "destination")?,
                arg_str_opt(args, "branch"),
            )
            .await
        }
        "git_status" => git_tools::status(state, arg_str(args, "repo_path")?).await,
        "git_add" => {
            git_tools::add(
                state,
                arg_str(args, "repo_path")?,
                &arg_str_array(args, "files"),
            )
            .await
        }
        "git_commit" => {
            git_tools::commit(
                state,
                arg_str(args, "repo_path")?,
                arg_str(args, "message")?,
            )
            .await
        }
        "git_push" => {
            git_tools::push(
                state,
                arg_str(args, "repo_path")?,
                arg_str_opt(args, "remote"),
                arg_str_opt(args, "branch"),
            )
            .await
        }
        "git_pull" => {
            git_tools::pull(
                state,
                arg_str(args, "repo_path")?,
                arg_str_opt(args, "remote"),
                arg_str_opt(args, "branch"),
            )
            .await
        }
        "git_branch_list" => git_tools::branch_list(state, arg_str(args, "repo_path")?).await,
        "git_checkout" => {
            git_tools::checkout(
                state,
                arg_str(args, "repo_path")?,
                arg_str(args, "branch")?,
                arg_bool(args, "create", false),
            )
            .await
        }
        "git_diff" => {
            git_tools::diff(
                state,
                arg_str(args, "repo_path")?,
                arg_bool(args, "staged", false),
            )
            .await
        }
        "git_log" => {
            git_tools::log(
                state,
                arg_str(args, "repo_path")?,
                arg_u32(args, "limit", 20),
            )
            .await
        }

        other => Err(format!("unknown tool: {other}")),
    }
}

pub fn tool_list() -> Value {
    json!([
        {
            "name": "fs_read_file",
            "description": "Read the full text content of a file within the repo storage area.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Path relative to the repo storage root." } },
                "required": ["path"]
            }
        },
        {
            "name": "fs_write_file",
            "description": "Write (create or overwrite) a text file within the repo storage area.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }
        },
        {
            "name": "fs_list_directory",
            "description": "List files and subdirectories at a given path within the repo storage area.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        },
        {
            "name": "fs_create_directory",
            "description": "Create a directory (and any missing parents) within the repo storage area.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        },
        {
            "name": "fs_delete_path",
            "description": "Delete a file or directory (recursively) within the repo storage area.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        },
        {
            "name": "fs_move_path",
            "description": "Move or rename a file or directory within the repo storage area.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string" },
                    "destination": { "type": "string" }
                },
                "required": ["source", "destination"]
            }
        },
        {
            "name": "fs_search_files",
            "description": "Recursively search for files/directories whose name contains the given substring.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory to search within." },
                    "pattern": { "type": "string", "description": "Substring to match against file/directory names (case-insensitive)." }
                },
                "required": ["path", "pattern"]
            }
        },
        {
            "name": "edit_file",
            "description": "Make a targeted single-location edit to an existing file WITHOUT rewriting the rest of it. old_str must match the current file content in exactly one place, including whitespace — if it matches zero or more than one location, the edit is rejected so you can widen old_str with more surrounding context. Always fs_read_file immediately before calling this, and construct old_str/new_str from that fresh read, not from an earlier read or from memory. Prefer this over fs_write_file for any change to an existing file that isn't a full-file rewrite.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_str": { "type": "string", "description": "Exact text to replace, unique within the file." },
                    "new_str": { "type": "string", "description": "Replacement text." }
                },
                "required": ["path", "old_str", "new_str"]
            }
        },
        {
            "name": "grep_content",
            "description": "Recursively search file CONTENTS (not filenames — see fs_search_files for that) for a substring, returning matching lines with file path and line number. Case-insensitive. Use this to locate where in a repo something is actually defined or referenced before editing it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory to search within." },
                    "pattern": { "type": "string", "description": "Substring to search for within file contents." }
                },
                "required": ["path", "pattern"]
            }
        },
        {
            "name": "exec_command",
            "description": "Run a shell command with its working directory inside the repo storage area (e.g. run tests, a linter, a build, or a read-only verification command like curl). Disabled unless the server operator has explicitly enabled it. Bounded by a hard timeout — a command that exceeds it is killed and reported as timed out, not left running. Prefer the dedicated fs_*/git_* tools over exec_command for anything they already cover (don't use exec_command to cat/edit/grep a file, or to run raw git plumbing that git_* already exposes).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string", "description": "Working directory for the command, relative to the repo storage root." },
                    "command": { "type": "string", "description": "Shell command to run via `bash -c`." },
                    "timeout_secs": { "type": "integer", "description": "Optional timeout override in seconds (default 120, max 900)." }
                },
                "required": ["repo_path", "command"]
            }
        },
        {
            "name": "git_clone",
            "description": "Clone a git repository into the repo storage area. If the target is the configured Forgejo host, credentials are applied automatically — use a plain https:// URL with no embedded token or username.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "destination": { "type": "string", "description": "Relative path to clone into." },
                    "branch": { "type": "string", "description": "Optional branch to check out." }
                },
                "required": ["url", "destination"]
            }
        },
        {
            "name": "git_status",
            "description": "Show the working tree status of a repo.",
            "inputSchema": {
                "type": "object",
                "properties": { "repo_path": { "type": "string" } },
                "required": ["repo_path"]
            }
        },
        {
            "name": "git_add",
            "description": "Stage files for commit. Omit 'files' to stage all changes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "files": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["repo_path"]
            }
        },
        {
            "name": "git_commit",
            "description": "Commit currently staged changes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "message": { "type": "string" }
                },
                "required": ["repo_path", "message"]
            }
        },
        {
            "name": "git_push",
            "description": "Push local commits to a remote.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "remote": { "type": "string", "description": "Defaults to 'origin'." },
                    "branch": { "type": "string" }
                },
                "required": ["repo_path"]
            }
        },
        {
            "name": "git_pull",
            "description": "Pull changes from a remote.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "remote": { "type": "string", "description": "Defaults to 'origin'." },
                    "branch": { "type": "string" }
                },
                "required": ["repo_path"]
            }
        },
        {
            "name": "git_branch_list",
            "description": "List local and remote branches.",
            "inputSchema": {
                "type": "object",
                "properties": { "repo_path": { "type": "string" } },
                "required": ["repo_path"]
            }
        },
        {
            "name": "git_checkout",
            "description": "Check out a branch, optionally creating it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "branch": { "type": "string" },
                    "create": { "type": "boolean", "description": "Create the branch if it doesn't exist." }
                },
                "required": ["repo_path", "branch"]
            }
        },
        {
            "name": "git_diff",
            "description": "Show diff of unstaged (or staged) changes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "staged": { "type": "boolean" }
                },
                "required": ["repo_path"]
            }
        },
        {
            "name": "git_log",
            "description": "Show recent commit history.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "limit": { "type": "integer", "description": "Number of commits to show. Defaults to 20." }
                },
                "required": ["repo_path"]
            }
        }
    ])
}
