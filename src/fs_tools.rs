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
