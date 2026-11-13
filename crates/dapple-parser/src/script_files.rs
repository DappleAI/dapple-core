//! Discovery of nushell script files from the `scripts/` directory.
//!
//! Scripts live in `scripts/<task-name>.nu` files within a guide.
//! The filename (without extension) becomes the task name, matching
//! the `name` attribute on the corresponding `:::nushell` directive.

use std::collections::HashMap;
use std::path::Path;

/// Discover all nushell script files from the `scripts/` directory.
///
/// Returns a map of script name → file content.
/// Returns an empty map if no `scripts/` directory exists.
pub fn discover_script_files(guide_dir: &Path) -> HashMap<String, String> {
    let scripts_dir = guide_dir.join("scripts");
    if !scripts_dir.is_dir() {
        return HashMap::new();
    }

    let mut scripts = HashMap::new();

    let entries = match std::fs::read_dir(&scripts_dir) {
        Ok(e) => e,
        Err(_) => return HashMap::new(),
    };

    for entry in entries.flatten() {
        let file_path = entry.path();
        if !file_path.is_file() {
            continue;
        }
        let Some(filename) = file_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(name) = filename.strip_suffix(".nu") else {
            continue;
        };
        if name.is_empty() {
            continue;
        }

        match std::fs::read_to_string(&file_path) {
            Ok(content) => {
                scripts.insert(name.to_string(), content);
            }
            Err(_) => continue,
        }
    }

    scripts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_script_files_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let scripts = discover_script_files(tmp.path());
        assert!(scripts.is_empty());
    }

    #[test]
    fn test_discover_script_files_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let scripts_dir = tmp.path().join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        fs::write(
            scripts_dir.join("my-task.nu"),
            "let input = $in\n{ payload: {}, signals: {} }",
        )
        .unwrap();

        let scripts = discover_script_files(tmp.path());
        assert_eq!(scripts.len(), 1);
        assert!(scripts.contains_key("my-task"));
        assert!(scripts["my-task"].contains("let input = $in"));
    }

    #[test]
    fn test_discover_script_files_multiple() {
        let tmp = tempfile::tempdir().unwrap();
        let scripts_dir = tmp.path().join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        fs::write(scripts_dir.join("alpha.nu"), "# alpha").unwrap();
        fs::write(scripts_dir.join("beta.nu"), "# beta").unwrap();

        let scripts = discover_script_files(tmp.path());
        assert_eq!(scripts.len(), 2);
        assert_eq!(scripts["alpha"], "# alpha");
        assert_eq!(scripts["beta"], "# beta");
    }

    #[test]
    fn test_discover_script_files_ignores_non_nu() {
        let tmp = tempfile::tempdir().unwrap();
        let scripts_dir = tmp.path().join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        fs::write(scripts_dir.join("my-task.nu"), "# script").unwrap();
        fs::write(scripts_dir.join("readme.md"), "# readme").unwrap();
        fs::write(scripts_dir.join("data.json"), "{}").unwrap();

        let scripts = discover_script_files(tmp.path());
        assert_eq!(scripts.len(), 1);
        assert!(scripts.contains_key("my-task"));
    }

    #[test]
    fn test_discover_script_files_no_directory() {
        let tmp = tempfile::tempdir().unwrap();
        // No scripts/ directory at all
        let scripts = discover_script_files(tmp.path());
        assert!(scripts.is_empty());
    }
}
