//! Mock GitAgent for testing.

#![cfg(test)]

use std::path::PathBuf;

use super::*;

/// A mock git agent with scripted responses.
pub struct MockGitAgent {
    pub is_repo: bool,
    pub root: PathBuf,
    pub branch: Option<String>,
    pub unstaged_files: Vec<ChangedFile>,
    pub staged_files: Vec<ChangedFile>,
    pub unstaged_diffs: Vec<FileDiff>,
    pub staged_diffs: Vec<FileDiff>,
    pub branch_diffs: Vec<FileDiff>,
}

impl MockGitAgent {
    pub fn in_repo() -> Self {
        Self {
            is_repo: true,
            root: PathBuf::from("/tmp/test-repo"),
            branch: Some("feature/test".to_string()),
            unstaged_files: vec![],
            staged_files: vec![],
            unstaged_diffs: vec![],
            staged_diffs: vec![],
            branch_diffs: vec![],
        }
    }

    pub fn not_a_repo() -> Self {
        Self {
            is_repo: false,
            root: PathBuf::from("/tmp/not-a-repo"),
            branch: None,
            unstaged_files: vec![],
            staged_files: vec![],
            unstaged_diffs: vec![],
            staged_diffs: vec![],
            branch_diffs: vec![],
        }
    }

    pub fn with_unstaged(mut self, files: Vec<ChangedFile>, diffs: Vec<FileDiff>) -> Self {
        self.unstaged_files = files;
        self.unstaged_diffs = diffs;
        self
    }

    pub fn with_staged(mut self, files: Vec<ChangedFile>, diffs: Vec<FileDiff>) -> Self {
        self.staged_files = files;
        self.staged_diffs = diffs;
        self
    }

    pub fn with_branch_diffs(mut self, diffs: Vec<FileDiff>) -> Self {
        self.branch_diffs = diffs;
        self
    }
}

impl GitAgent for MockGitAgent {
    fn is_repo(&self) -> bool {
        self.is_repo
    }

    fn repo_root(&self) -> Result<PathBuf, GitError> {
        if self.is_repo {
            Ok(self.root.clone())
        } else {
            Err(GitError::NotARepo)
        }
    }

    fn current_branch(&self) -> Result<Option<String>, GitError> {
        if !self.is_repo {
            return Err(GitError::NotARepo);
        }
        Ok(self.branch.clone())
    }

    fn changed_files_unstaged(&self) -> Result<Vec<ChangedFile>, GitError> {
        if !self.is_repo {
            return Err(GitError::NotARepo);
        }
        Ok(self.unstaged_files.clone())
    }

    fn changed_files_staged(&self) -> Result<Vec<ChangedFile>, GitError> {
        if !self.is_repo {
            return Err(GitError::NotARepo);
        }
        Ok(self.staged_files.clone())
    }

    fn diff_unstaged(&self) -> Result<Vec<FileDiff>, GitError> {
        if !self.is_repo {
            return Err(GitError::NotARepo);
        }
        Ok(self.unstaged_diffs.clone())
    }

    fn diff_staged(&self) -> Result<Vec<FileDiff>, GitError> {
        if !self.is_repo {
            return Err(GitError::NotARepo);
        }
        Ok(self.staged_diffs.clone())
    }

    fn diff_all(&self) -> Result<Vec<FileDiff>, GitError> {
        if !self.is_repo {
            return Err(GitError::NotARepo);
        }
        // Combine unstaged + staged, dedup by path (same as real impl)
        let mut combined = self.unstaged_diffs.clone();
        let unstaged_paths: std::collections::HashSet<String> =
            combined.iter().map(|d| d.path.clone()).collect();
        for diff in &self.staged_diffs {
            if !unstaged_paths.contains(&diff.path) {
                combined.push(diff.clone());
            }
        }
        Ok(combined)
    }

    fn diff_branch(&self, _base: &str) -> Result<Vec<FileDiff>, GitError> {
        if !self.is_repo {
            return Err(GitError::NotARepo);
        }
        Ok(self.branch_diffs.clone())
    }

    fn stage_files(&self, _paths: &[&str]) -> Result<(), GitError> {
        if !self.is_repo {
            return Err(GitError::NotARepo);
        }
        Ok(())
    }

    fn commit(&self, _message: &str) -> Result<String, GitError> {
        if !self.is_repo {
            return Err(GitError::NotARepo);
        }
        Ok("abc123def456".to_string())
    }
}

/// Helper to create a simple FileDiff for testing.
pub fn make_file_diff(path: &str, status: FileStatus, hunk_content: &str) -> FileDiff {
    FileDiff {
        path: path.to_string(),
        status,
        hunks: vec![DiffHunk {
            old_start: 1,
            old_lines: 3,
            new_start: 1,
            new_lines: 5,
            content: hunk_content.to_string(),
        }],
    }
}
