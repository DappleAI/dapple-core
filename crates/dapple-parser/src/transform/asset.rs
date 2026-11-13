//! Asset file collection from the guide directory.
//!
//! Reads the bytes of each asset file discovered by the parser, infers
//! MIME types from file extensions, and produces `AssetFileEntry` values
//! for the ingest plan.

use super::AssetFileEntry;
use std::path::Path;

/// Collect asset file entries from the guide directory.
///
/// For each relative path in `asset_paths`, reads the file bytes and
/// infers the MIME type. Files that cannot be read produce a warning
/// and are skipped.
pub fn collect_asset_files(
    guide_dir: &Path,
    asset_paths: &[String],
    warnings: &mut Vec<String>,
) -> Vec<AssetFileEntry> {
    let mut entries = Vec::new();

    for relative_path in asset_paths {
        let full_path = guide_dir.join(relative_path);

        match std::fs::read(&full_path) {
            Ok(data) => {
                let mime_type = mime_from_extension(&full_path);
                entries.push(AssetFileEntry {
                    relative_path: relative_path.clone(),
                    mime_type,
                    data,
                });
            }
            Err(e) => {
                warnings.push(format!(
                    "Could not read asset file '{}': {}",
                    relative_path, e
                ));
            }
        }
    }

    entries
}

/// Infer MIME type from file extension.
///
/// Returns a reasonable default for common file types used in guide content.
fn mime_from_extension(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        // Images
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        // Documents
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        // Data
        "json" => "application/json",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "yaml" | "yml" => "text/yaml",
        // Audio
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        // Video
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        // Archives
        "zip" => "application/zip",
        "gz" | "gzip" => "application/gzip",
        "tar" => "application/x-tar",
        // Text
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        // Default
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_collect_asset_files() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("assets/images")).unwrap();
        fs::write(
            dir.path().join("assets/images").join("card.jpg"),
            b"fake-jpg-data",
        )
        .unwrap();
        fs::write(dir.path().join("data.csv"), b"a,b,c\n1,2,3").unwrap();

        let asset_paths = vec!["assets/images/card.jpg".to_string(), "data.csv".to_string()];

        let mut warnings = Vec::new();
        let entries = collect_asset_files(dir.path(), &asset_paths, &mut warnings);

        assert_eq!(entries.len(), 2);
        assert!(warnings.is_empty());

        let jpg = entries
            .iter()
            .find(|e| e.relative_path == "assets/images/card.jpg")
            .unwrap();
        assert_eq!(jpg.mime_type, "image/jpeg");
        assert_eq!(jpg.data, b"fake-jpg-data");

        let csv = entries
            .iter()
            .find(|e| e.relative_path == "data.csv")
            .unwrap();
        assert_eq!(csv.mime_type, "text/csv");
    }

    #[test]
    fn test_collect_asset_files_missing_file() {
        let dir = TempDir::new().unwrap();
        let asset_paths = vec!["nonexistent.png".to_string()];

        let mut warnings = Vec::new();
        let entries = collect_asset_files(dir.path(), &asset_paths, &mut warnings);

        assert!(entries.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("nonexistent.png"));
    }

    #[test]
    fn test_mime_from_extension() {
        use std::path::PathBuf;

        assert_eq!(
            mime_from_extension(&PathBuf::from("test.jpg")),
            "image/jpeg"
        );
        assert_eq!(mime_from_extension(&PathBuf::from("test.png")), "image/png");
        assert_eq!(
            mime_from_extension(&PathBuf::from("test.svg")),
            "image/svg+xml"
        );
        assert_eq!(
            mime_from_extension(&PathBuf::from("test.pdf")),
            "application/pdf"
        );
        assert_eq!(mime_from_extension(&PathBuf::from("test.csv")), "text/csv");
        assert_eq!(
            mime_from_extension(&PathBuf::from("test.unknown")),
            "application/octet-stream"
        );
        assert_eq!(
            mime_from_extension(&PathBuf::from("no_extension")),
            "application/octet-stream"
        );
    }
}
