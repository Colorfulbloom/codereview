//! Review sources — where the code under review comes from.
//!
//! The reviewer is fundamentally diff-based, but [`ReviewTarget::Path`] lets a
//! user point the tool at an arbitrary file or directory (a module, a theme, or
//! any loose code) and have every supported file reviewed *as-is*, independent
//! of git state. Such files are modeled as fully-added diffs so the rest of the
//! pipeline (language detection, agents, prompts) is unchanged.

use std::path::Path;

use crate::git::{DiffHunk, FileDiff, FileStatus};
use crate::language;

/// What a review should look at.
#[derive(Debug, Clone, Copy)]
pub enum ReviewTarget<'a> {
    /// All uncommitted changes: HEAD → working tree (staged + unstaged + untracked).
    WorkingTree,
    /// Changes of the current HEAD relative to a base ref (branch/commit/tag).
    Ref(&'a str),
    /// Every supported file under a path, reviewed as-is (not a diff).
    Path(&'a Path),
}

/// Directory names never worth walking into for a code review.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "vendor",
    "dist",
    "build",
    ".ddev",
    ".lando",
];

/// Per-file size ceiling. Files larger than this are skipped so a single
/// generated blob (minified CSS, a compiled artifact) can't blow the model's
/// context window. Hand-written source is virtually always far smaller.
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("Path not found: {0}")]
    NotFound(String),

    #[error("Failed to read {0}: {1}")]
    Io(String, std::io::Error),
}

/// Read a file or directory into a set of fully-added [`FileDiff`]s.
///
/// Only files whose extension maps to a supported language are included;
/// unsupported, binary/non-UTF-8, empty, and oversized files are skipped.
/// Directories are walked recursively, skipping [`SKIP_DIRS`] and dotfiles.
/// Results are sorted by path for deterministic output.
pub fn read_path_as_diffs(root: &Path) -> Result<Vec<FileDiff>, SourceError> {
    if !root.exists() {
        return Err(SourceError::NotFound(root.display().to_string()));
    }

    let mut diffs = Vec::new();
    if root.is_file() {
        if let Some(fd) = file_to_diff(root)? {
            diffs.push(fd);
        }
    } else {
        walk(root, &mut diffs)?;
    }

    diffs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(diffs)
}

fn walk(dir: &Path, out: &mut Vec<FileDiff>) -> Result<(), SourceError> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| SourceError::Io(dir.display().to_string(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| SourceError::Io(dir.display().to_string(), e))?;
        let path = entry.path();
        // `file_type` does not follow symlinks (lstat), so symlinked
        // directories are treated as non-dirs and never recursed into —
        // avoiding cycles.
        let file_type = entry
            .file_type()
            .map_err(|e| SourceError::Io(path.display().to_string(), e))?;

        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk(&path, out)?;
        } else if file_type.is_file()
            && let Some(fd) = file_to_diff(&path)?
        {
            out.push(fd);
        }
    }

    Ok(())
}

/// Build a fully-added diff for one file, or `None` if it should be skipped.
fn file_to_diff(path: &Path) -> Result<Option<FileDiff>, SourceError> {
    let path_str = path.to_string_lossy().into_owned();

    // Only review file types the agents understand.
    if language::detect_language(&path_str).is_none() {
        return Ok(None);
    }

    let metadata =
        std::fs::metadata(path).map_err(|e| SourceError::Io(path_str.clone(), e))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Ok(None);
    }

    // Non-UTF-8 (binary) files can't be reviewed as text — skip them.
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    if content.is_empty() {
        return Ok(None);
    }

    // Present every line as an addition, mirroring how git diffs of a brand-new
    // file look to the rest of the pipeline.
    let mut body = String::with_capacity(content.len() + content.len() / 40 + 1);
    let mut new_lines = 0u32;
    for line in content.split_inclusive('\n') {
        body.push('+');
        body.push_str(line);
        new_lines += 1;
    }

    Ok(Some(FileDiff {
        path: path_str,
        status: FileStatus::Added,
        hunks: vec![DiffHunk {
            old_start: 0,
            old_lines: 0,
            new_start: 1,
            new_lines,
            content: body,
        }],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn single_file_becomes_added_diff() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("foo.php");
        fs::write(&file, "<?php\necho 1;\n").unwrap();

        let diffs = read_path_as_diffs(&file).unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, FileStatus::Added);
        assert_eq!(diffs[0].hunks.len(), 1);
        assert_eq!(diffs[0].hunks[0].new_lines, 2);
        assert!(diffs[0].hunks[0].content.contains("+<?php"));
        assert!(diffs[0].hunks[0].content.contains("+echo 1;"));
    }

    #[test]
    fn directory_is_walked_recursively() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.module"), "<?php\n").unwrap();
        let sub = dir.path().join("src/Controller");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("B.php"), "<?php\n").unwrap();

        let diffs = read_path_as_diffs(dir.path()).unwrap();

        let names: Vec<String> = diffs
            .iter()
            .map(|d| {
                Path::new(&d.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(names.contains(&"a.module".to_string()));
        assert!(names.contains(&"B.php".to_string()));
    }

    #[test]
    fn unsupported_and_binary_files_are_skipped() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("keep.php"), "<?php\n").unwrap();
        fs::write(dir.path().join("notes.txt"), "ignore me\n").unwrap();
        fs::write(dir.path().join("data.bin"), [0u8, 159, 146, 150]).unwrap();

        let diffs = read_path_as_diffs(dir.path()).unwrap();

        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].path.ends_with("keep.php"));
    }

    #[test]
    fn skip_dirs_are_not_walked() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("real.php"), "<?php\n").unwrap();
        let vendor = dir.path().join("vendor/pkg");
        fs::create_dir_all(&vendor).unwrap();
        fs::write(vendor.join("dep.php"), "<?php\n").unwrap();

        let diffs = read_path_as_diffs(dir.path()).unwrap();

        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].path.ends_with("real.php"));
    }

    #[test]
    fn oversized_files_are_skipped() {
        let dir = TempDir::new().unwrap();
        let big = "a".repeat((MAX_FILE_BYTES + 1) as usize);
        fs::write(dir.path().join("huge.css"), big).unwrap();
        fs::write(dir.path().join("small.css"), "body{}\n").unwrap();

        let diffs = read_path_as_diffs(dir.path()).unwrap();

        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].path.ends_with("small.css"));
    }

    #[test]
    fn missing_path_errors() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(matches!(
            read_path_as_diffs(&missing),
            Err(SourceError::NotFound(_))
        ));
    }
}
