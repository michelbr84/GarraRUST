//! Path normalization utilities - GAR-245

use std::path::Path;

/// Normalize a path to use forward slashes (cross-platform)
///
/// This converts Windows-style backslashes to forward slashes
/// and handles other path edge cases.
pub fn normalize_path(path: &str) -> String {
    // Replace backslashes with forward slashes
    path.replace('\\', "/")
}

/// Normalize a path and resolve . and .. components
pub fn normalize_path_components(path: &str) -> String {
    let normalized = normalize_path(path);
    let parts: Vec<&str> = normalized.split('/').collect();
    let mut result = Vec::new();

    for part in parts {
        match part {
            "" | "." => continue,
            ".." => {
                result.pop();
            }
            _ => result.push(part),
        }
    }

    if result.is_empty() {
        String::from("/")
    } else {
        result.join("/")
    }
}

/// Check if a path attempts to escape the root directory
pub fn is_path_traversal(path: &str) -> bool {
    let normalized = normalize_path(path);
    normalized.contains("..")
}

/// Convert an absolute path to a relative path from a root
pub fn relative_to(path: &str, root: &str) -> Option<String> {
    let path_norm = normalize_path(path);
    let root_norm = normalize_path(root);

    if path_norm.starts_with(&root_norm) {
        let relative = &path_norm[root_norm.len()..];
        Some(relative.trim_start_matches('/').to_string())
    } else {
        None
    }
}

/// Get the file extension from a path
pub fn extension(path: &str) -> Option<&str> {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
}

/// Get the file name from a path
pub fn file_name(path: &str) -> Option<&str> {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
}

/// Get the parent directory from a path
pub fn parent(path: &str) -> Option<String> {
    Path::new(path)
        .parent()
        .map(|p| normalize_path(&p.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("src\\main.rs"), "src/main.rs");
        assert_eq!(normalize_path("src/main.rs"), "src/main.rs");
    }

    #[test]
    fn test_normalize_components() {
        assert_eq!(normalize_path_components("src/./main.rs"), "src/main.rs");
        assert_eq!(normalize_path_components("src/../main.rs"), "main.rs");
    }

    #[test]
    fn test_path_traversal() {
        assert!(is_path_traversal("../etc/passwd"));
        assert!(is_path_traversal("src/../../etc/passwd"));
        assert!(!is_path_traversal("src/main.rs"));
    }
}
