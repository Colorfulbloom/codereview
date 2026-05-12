//! Git operations module — diff generation, branch detection, file status.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Status of a changed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

impl std::fmt::Display for FileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileStatus::Added => write!(f, "added"),
            FileStatus::Modified => write!(f, "modified"),
            FileStatus::Deleted => write!(f, "deleted"),
            FileStatus::Renamed => write!(f, "renamed"),
        }
    }
}

/// A file that has changed relative to some reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub status: FileStatus,
}

/// A contiguous block of changes within a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    /// Starting line in the old file.
    pub old_start: u32,
    /// Number of lines in the old file.
    pub old_lines: u32,
    /// Starting line in the new file.
    pub new_start: u32,
    /// Number of lines in the new file.
    pub new_lines: u32,
    /// The patch text for this hunk (unified diff format).
    pub content: String,
}

/// Diff for a single file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub status: FileStatus,
    pub hunks: Vec<DiffHunk>,
}

/// Error type for git operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Not a git repository")]
    NotARepo,

    #[error("Git error: {0}")]
    Git2(#[from] git2::Error),

    #[error("Branch not found: {0}")]
    BranchNotFound(String),

    #[error("{0}")]
    Other(String),
}

/// Trait for git operations needed by the review pipeline.
///
/// Designed for dependency injection — use `MockGitAgent` in tests,
/// `LiveGitAgent` in production.
pub trait GitAgent: Send + Sync {
    /// Whether the current directory is inside a git repository.
    fn is_repo(&self) -> bool;

    /// The root directory of the git repository.
    fn repo_root(&self) -> Result<PathBuf, GitError>;

    /// Current branch name (e.g., "feature/my-branch"), or None if detached HEAD.
    fn current_branch(&self) -> Result<Option<String>, GitError>;

    /// Files changed in the working tree (unstaged changes).
    fn changed_files_unstaged(&self) -> Result<Vec<ChangedFile>, GitError>;

    /// Files changed in the index (staged changes).
    fn changed_files_staged(&self) -> Result<Vec<ChangedFile>, GitError>;

    /// Diff of unstaged changes (working tree vs HEAD).
    fn diff_unstaged(&self) -> Result<Vec<FileDiff>, GitError>;

    /// Diff of staged changes (index vs HEAD).
    fn diff_staged(&self) -> Result<Vec<FileDiff>, GitError>;

    /// Diff of ALL uncommitted changes: HEAD → working tree.
    /// Combines staged + unstaged into one view showing the current state.
    fn diff_all(&self) -> Result<Vec<FileDiff>, GitError>;

    /// Diff of current HEAD vs a base branch (e.g., "main").
    fn diff_branch(&self, base: &str) -> Result<Vec<FileDiff>, GitError>;

    /// Stage specific files by path.
    fn stage_files(&self, paths: &[&str]) -> Result<(), GitError>;

    /// Create a commit with the given message. Only commits staged changes.
    fn commit(&self, message: &str) -> Result<String, GitError>;
}

/// Live implementation using git2.
pub struct LiveGitAgent {
    path: PathBuf,
}

impl LiveGitAgent {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    // W1: Preserve the original git2 error instead of discarding it
    fn open_repo(&self) -> Result<git2::Repository, GitError> {
        git2::Repository::discover(&self.path).map_err(|e| {
            if e.code() == git2::ErrorCode::NotFound {
                GitError::NotARepo
            } else {
                GitError::Git2(e)
            }
        })
    }

    fn delta_to_status(delta: git2::Delta) -> FileStatus {
        match delta {
            git2::Delta::Added | git2::Delta::Untracked => FileStatus::Added,
            git2::Delta::Deleted => FileStatus::Deleted,
            git2::Delta::Renamed | git2::Delta::Copied => FileStatus::Renamed,
            _ => FileStatus::Modified,
        }
    }

    // W2: Use iterator instead of manual index loop
    fn extract_changed_files(diff: &git2::Diff) -> Vec<ChangedFile> {
        diff.deltas()
            .map(|delta| {
                let path = delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .map(|p| p.to_string_lossy().into_owned()) // S3: into_owned
                    .unwrap_or_default();
                ChangedFile {
                    path,
                    status: Self::delta_to_status(delta.status()),
                }
            })
            .collect()
    }

    fn extract_file_diffs(diff: &git2::Diff) -> Result<Vec<FileDiff>, GitError> {
        let mut file_diffs: Vec<FileDiff> = Vec::new();

        diff.print(git2::DiffFormat::Patch, |delta, hunk, line| {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().into_owned()) // S3: into_owned
                .unwrap_or_default();

            let status = Self::delta_to_status(delta.status());

            // Find or create the FileDiff for this path
            let file_diff = if let Some(fd) = file_diffs.iter_mut().find(|fd| fd.path == path) {
                fd
            } else {
                file_diffs.push(FileDiff {
                    path,
                    status,
                    hunks: Vec::new(),
                });
                file_diffs.last_mut().unwrap()
            };

            if let Some(h) = hunk {
                let hunk_start = h.new_start();
                let needs_new_hunk = file_diff
                    .hunks
                    .last()
                    .is_none_or(|last| last.new_start != hunk_start);

                if needs_new_hunk {
                    file_diff.hunks.push(DiffHunk {
                        old_start: h.old_start(),
                        old_lines: h.old_lines(),
                        new_start: h.new_start(),
                        new_lines: h.new_lines(),
                        content: String::new(),
                    });
                }
            }

            if let Some(current_hunk) = file_diff.hunks.last_mut() {
                let prefix = match line.origin() {
                    '+' => "+",
                    '-' => "-",
                    ' ' => " ",
                    _ => "",
                };
                if !prefix.is_empty() {
                    let text = std::str::from_utf8(line.content()).unwrap_or("");
                    current_hunk.content.push_str(prefix);
                    current_hunk.content.push_str(text);
                }
            }

            true
        })?;

        Ok(file_diffs)
    }

    fn head_tree(repo: &git2::Repository) -> Result<Option<git2::Tree<'_>>, GitError> {
        match repo.head() {
            Ok(head) => {
                let commit = head.peel_to_commit()?;
                Ok(Some(commit.tree()?))
            }
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => Ok(None),
            Err(e) => Err(GitError::Git2(e)),
        }
    }
}

impl GitAgent for LiveGitAgent {
    fn is_repo(&self) -> bool {
        self.open_repo().is_ok()
    }

    fn repo_root(&self) -> Result<PathBuf, GitError> {
        let repo = self.open_repo()?;
        repo.workdir()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| GitError::Other("Bare repository has no working directory".into()))
    }

    fn current_branch(&self) -> Result<Option<String>, GitError> {
        let repo = self.open_repo()?;
        match repo.head() {
            Ok(head) => {
                if head.is_branch() {
                    Ok(head.shorthand().map(String::from))
                } else {
                    Ok(None) // detached HEAD
                }
            }
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => Ok(None),
            Err(e) => Err(GitError::Git2(e)),
        }
    }

    fn changed_files_unstaged(&self) -> Result<Vec<ChangedFile>, GitError> {
        let repo = self.open_repo()?;
        let diff = repo.diff_index_to_workdir(None, None)?;
        Ok(Self::extract_changed_files(&diff))
    }

    fn changed_files_staged(&self) -> Result<Vec<ChangedFile>, GitError> {
        let repo = self.open_repo()?;
        let head_tree = Self::head_tree(&repo)?;
        let diff = repo.diff_tree_to_index(head_tree.as_ref(), None, None)?;
        Ok(Self::extract_changed_files(&diff))
    }

    fn diff_unstaged(&self) -> Result<Vec<FileDiff>, GitError> {
        let repo = self.open_repo()?;
        let diff = repo.diff_index_to_workdir(None, None)?;
        Self::extract_file_diffs(&diff)
    }

    fn diff_staged(&self) -> Result<Vec<FileDiff>, GitError> {
        let repo = self.open_repo()?;
        let head_tree = Self::head_tree(&repo)?;
        let diff = repo.diff_tree_to_index(head_tree.as_ref(), None, None)?;
        Self::extract_file_diffs(&diff)
    }

    fn diff_all(&self) -> Result<Vec<FileDiff>, GitError> {
        let repo = self.open_repo()?;
        let head_tree = Self::head_tree(&repo)?;
        // diff_tree_to_workdir_with_index: HEAD → working tree (staged + unstaged combined)
        let diff = repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), None)?;
        Self::extract_file_diffs(&diff)
    }

    fn diff_branch(&self, base: &str) -> Result<Vec<FileDiff>, GitError> {
        let repo = self.open_repo()?;

        let base_ref = repo
            .find_branch(base, git2::BranchType::Local)
            .or_else(|_| repo.find_branch(base, git2::BranchType::Remote))
            .map_err(|_| GitError::BranchNotFound(base.to_string()))?;
        let base_commit = base_ref.get().peel_to_commit()?;
        let base_tree = base_commit.tree()?;

        let head_tree = Self::head_tree(&repo)?
            .ok_or_else(|| GitError::Other("No commits on current branch".into()))?;

        let diff = repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)?;
        Self::extract_file_diffs(&diff)
    }

    fn stage_files(&self, paths: &[&str]) -> Result<(), GitError> {
        let repo = self.open_repo()?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| GitError::Other("Bare repository".into()))?;
        let mut index = repo.index()?;

        for path in paths {
            let p = std::path::Path::new(path);
            if workdir.join(p).exists() {
                index.add_path(p)?;
            } else {
                // File was deleted — remove from index
                index.remove_path(p)?;
            }
        }

        index.write()?;
        Ok(())
    }

    fn commit(&self, message: &str) -> Result<String, GitError> {
        let repo = self.open_repo()?;
        let mut index = repo.index()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let sig = repo.signature().map_err(|e| {
            GitError::Other(format!(
                "Git user not configured. Run: git config user.name \"Your Name\" && git config user.email \"you@example.com\". Error: {e}"
            ))
        })?;

        let parent = match repo.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => None,
            Err(e) => return Err(GitError::Git2(e)),
        };

        let parents: Vec<&git2::Commit> = parent.as_ref().map_or(vec![], |p| vec![p]);

        let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;

        Ok(oid.to_string())
    }
}

#[cfg(test)]
pub mod testutil;

#[cfg(test)]
mod tests;
