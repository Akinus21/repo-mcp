use std::path::{Path, PathBuf};

use crate::AppState;

/// Resolve a user-supplied relative path against base_dir, refusing any
/// path that would escape it (e.g. via `..` or an absolute path).
pub fn resolve(state: &AppState, user_path: &str) -> Result<PathBuf, String> {
    let base = state
        .base_dir
        .canonicalize()
        .map_err(|e| format!("base_dir error: {e}"))?;

    let joined = if Path::new(user_path).is_absolute() {
        PathBuf::from(user_path)
    } else {
        base.join(user_path)
    };

    // Canonicalize what we can; for paths that don't exist yet (e.g. a file
    // we're about to create), canonicalize the parent instead.
    let candidate = if joined.exists() {
        joined.canonicalize().map_err(|e| e.to_string())?
    } else {
        let parent = joined
            .parent()
            .ok_or_else(|| "invalid path".to_string())?;
        let parent_canon = if parent.exists() {
            parent.canonicalize().map_err(|e| e.to_string())?
        } else {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            parent.canonicalize().map_err(|e| e.to_string())?
        };
        parent_canon.join(joined.file_name().unwrap_or_default())
    };

    if !candidate.starts_with(&base) {
        return Err(format!(
            "path escapes allowed base directory: {}",
            user_path
        ));
    }
    Ok(candidate)
}

pub fn read_file(state: &AppState, path: &str) -> Result<String, String> {
    let p = resolve(state, path)?;
    std::fs::read_to_string(&p).map_err(|e| format!("read failed: {e}"))
}

pub fn write_file(state: &AppState, path: &str, content: &str) -> Result<String, String> {
    let p = resolve(state, path)?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, content).map_err(|e| format!("write failed: {e}"))?;
    Ok(format!("wrote {} bytes to {}", content.len(), path))
}

pub fn list_directory(state: &AppState, path: &str) -> Result<String, String> {
    let p = resolve(state, path)?;
    let mut entries = vec![];
    for entry in std::fs::read_dir(&p).map_err(|e| format!("list failed: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        let kind = if file_type.is_dir() { "dir" } else { "file" };
        entries.push(format!("[{}] {}", kind, entry.file_name().to_string_lossy()));
    }
    entries.sort();
    Ok(entries.join("\n"))
}

pub fn create_directory(state: &AppState, path: &str) -> Result<String, String> {
    let p = resolve(state, path)?;
    std::fs::create_dir_all(&p).map_err(|e| format!("mkdir failed: {e}"))?;
    Ok(format!("created directory {}", path))
}

pub fn delete_path(state: &AppState, path: &str) -> Result<String, String> {
    let p = resolve(state, path)?;
    if p.is_dir() {
        std::fs::remove_dir_all(&p).map_err(|e| format!("rmdir failed: {e}"))?;
    } else {
        std::fs::remove_file(&p).map_err(|e| format!("rm failed: {e}"))?;
    }
    Ok(format!("deleted {}", path))
}

pub fn move_path(state: &AppState, src: &str, dest: &str) -> Result<String, String> {
    let s = resolve(state, src)?;
    let d = resolve(state, dest)?;
    std::fs::rename(&s, &d).map_err(|e| format!("move failed: {e}"))?;
    Ok(format!("moved {} -> {}", src, dest))
}

/// Targeted single-match replace. Rejects if `old_str` matches zero or
/// more than one location in the file — the caller must widen `old_str`
/// with surrounding context to make it unique. This exists specifically
/// so small edits never require reconstructing or overwriting a whole
/// file: `write_file` should be reserved for genuinely new files or
/// full-file rewrites, not incremental fixes.
pub fn edit_file(state: &AppState, path: &str, old_str: &str, new_str: &str) -> Result<String, String> {
    let p = resolve(state, path)?;
    let content = std::fs::read_to_string(&p).map_err(|e| format!("read failed: {e}"))?;

    let matches = content.matches(old_str).count();
    if matches == 0 {
        return Err(format!(
            "old_str not found in {path} — re-read the file and copy the exact text, including whitespace"
        ));
    }
    if matches > 1 {
        return Err(format!(
            "old_str matches {matches} locations in {path} — widen old_str with surrounding context so it matches exactly one place"
        ));
    }

    let updated = content.replacen(old_str, new_str, 1);
    std::fs::write(&p, &updated).map_err(|e| format!("write failed: {e}"))?;
    Ok(format!(
        "edited {path}: replaced 1 occurrence ({} bytes -> {} bytes)",
        content.len(),
        updated.len()
    ))
}

/// Grep-style content search across a directory tree — distinct from
/// `search_files`, which matches on filename only. Returns matching
/// lines with file path and line number, capped to avoid flooding the
/// response on a broad pattern.
pub fn grep_content(state: &AppState, path: &str, pattern: &str) -> Result<String, String> {
    let p = resolve(state, path)?;
    let base = state.base_dir.canonicalize().map_err(|e| e.to_string())?;
    let pattern_lower = pattern.to_lowercase();
    let mut matches = vec![];
    const MAX_MATCHES: usize = 200;

    for entry in walkdir::WalkDir::new(&p)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let rel = match entry.path().strip_prefix(&base) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };
        let text = match std::fs::read_to_string(entry.path()) {
            Ok(t) => t,
            Err(_) => continue, // skip binary/unreadable files
        };
        for (i, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&pattern_lower) {
                matches.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                if matches.len() >= MAX_MATCHES {
                    matches.push(format!("... capped at {MAX_MATCHES} matches, narrow the pattern or path"));
                    return Ok(matches.join("\n"));
                }
            }
        }
    }

    if matches.is_empty() {
        Ok("no matches".to_string())
    } else {
        Ok(matches.join("\n"))
    }
}

pub fn search_files(state: &AppState, path: &str, pattern: &str) -> Result<String, String> {
    let p = resolve(state, path)?;
    let mut matches = vec![];
    for entry in walkdir::WalkDir::new(&p)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.contains(&pattern.to_lowercase()) {
            if let Ok(rel) = entry.path().strip_prefix(&state.base_dir.canonicalize().unwrap()) {
                matches.push(rel.to_string_lossy().to_string());
            }
        }
    }
    matches.sort();
    if matches.is_empty() {
        Ok("no matches".to_string())
    } else {
        Ok(matches.join("\n"))
    }
}
