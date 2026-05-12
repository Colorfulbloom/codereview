//! Tests for the git module.
//!
//! Trait contract tests use MockGitAgent.
//! Integration tests use a real temp git repo with LiveGitAgent.

use std::path::Path;

use super::*;
use testutil::*;

// ── Trait contract tests (MockGitAgent) ──

mod trait_contract {
    use super::*;

    #[test]
    fn not_a_repo_returns_errors() {
        let agent = MockGitAgent::not_a_repo();

        assert!(!agent.is_repo());
        assert!(agent.repo_root().is_err());
        assert!(agent.current_branch().is_err());
        assert!(agent.changed_files_unstaged().is_err());
        assert!(agent.changed_files_staged().is_err());
        assert!(agent.diff_unstaged().is_err());
        assert!(agent.diff_staged().is_err());
        assert!(agent.diff_branch("main").is_err());
    }

    #[test]
    fn in_repo_returns_branch() {
        let agent = MockGitAgent::in_repo();

        assert!(agent.is_repo());
        assert_eq!(
            agent.current_branch().unwrap(),
            Some("feature/test".to_string())
        );
    }

    #[test]
    fn empty_repo_has_no_changes() {
        let agent = MockGitAgent::in_repo();

        assert!(agent.changed_files_unstaged().unwrap().is_empty());
        assert!(agent.changed_files_staged().unwrap().is_empty());
        assert!(agent.diff_unstaged().unwrap().is_empty());
        assert!(agent.diff_staged().unwrap().is_empty());
        assert!(agent.diff_branch("main").unwrap().is_empty());
    }

    #[test]
    fn unstaged_changes_returned() {
        let agent = MockGitAgent::in_repo().with_unstaged(
            vec![ChangedFile {
                path: "src/main.rs".to_string(),
                status: FileStatus::Modified,
            }],
            vec![make_file_diff(
                "src/main.rs",
                FileStatus::Modified,
                "+fn new_function() {}",
            )],
        );

        let files = agent.changed_files_unstaged().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].status, FileStatus::Modified);

        let diffs = agent.diff_unstaged().unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "src/main.rs");
        assert!(diffs[0].hunks[0].content.contains("+fn new_function"));
    }

    #[test]
    fn staged_changes_returned() {
        let agent = MockGitAgent::in_repo().with_staged(
            vec![ChangedFile {
                path: "README.md".to_string(),
                status: FileStatus::Added,
            }],
            vec![make_file_diff(
                "README.md",
                FileStatus::Added,
                "+# My Project",
            )],
        );

        let files = agent.changed_files_staged().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Added);

        let diffs = agent.diff_staged().unwrap();
        assert_eq!(diffs.len(), 1);
    }

    #[test]
    fn branch_diff_returned() {
        let agent = MockGitAgent::in_repo().with_branch_diffs(vec![
            make_file_diff("src/lib.rs", FileStatus::Modified, "+pub mod new_module;"),
            make_file_diff("src/new_module.rs", FileStatus::Added, "+pub fn hello() {}"),
        ]);

        let diffs = agent.diff_branch("main").unwrap();
        assert_eq!(diffs.len(), 2);
        assert_eq!(diffs[0].path, "src/lib.rs");
        assert_eq!(diffs[1].path, "src/new_module.rs");
        assert_eq!(diffs[1].status, FileStatus::Added);
    }
}

// ── Data type tests ──

mod types {
    use super::*;

    #[test]
    fn file_status_display() {
        assert_eq!(FileStatus::Added.to_string(), "added");
        assert_eq!(FileStatus::Modified.to_string(), "modified");
        assert_eq!(FileStatus::Deleted.to_string(), "deleted");
        assert_eq!(FileStatus::Renamed.to_string(), "renamed");
    }

    #[test]
    fn changed_file_serializes() {
        let file = ChangedFile {
            path: "src/main.rs".to_string(),
            status: FileStatus::Modified,
        };
        let json = serde_json::to_string(&file).unwrap();
        assert!(json.contains("src/main.rs"));
        assert!(json.contains("Modified"));
    }

    #[test]
    fn file_diff_with_multiple_hunks() {
        let diff = FileDiff {
            path: "src/lib.rs".to_string(),
            status: FileStatus::Modified,
            hunks: vec![
                DiffHunk {
                    old_start: 1,
                    old_lines: 3,
                    new_start: 1,
                    new_lines: 5,
                    content: "+use std::io;\n+use std::fs;".to_string(),
                },
                DiffHunk {
                    old_start: 20,
                    old_lines: 2,
                    new_start: 22,
                    new_lines: 4,
                    content: "+fn new_helper() {\n+    todo!()\n+}".to_string(),
                },
            ],
        };

        assert_eq!(diff.hunks.len(), 2);
        assert_eq!(diff.hunks[0].new_start, 1);
        assert_eq!(diff.hunks[1].old_start, 20);
    }

    #[test]
    fn diff_hunk_line_counts() {
        let hunk = DiffHunk {
            old_start: 10,
            old_lines: 3,
            new_start: 10,
            new_lines: 7,
            content: "context\n-removed\n+added line 1\n+added line 2\n+added line 3\n+added line 4\ncontext".to_string(),
        };

        // 7 new lines - 3 old lines = 4 net added lines
        assert_eq!(hunk.new_lines as i32 - hunk.old_lines as i32, 4);
    }
}

// ── Integration tests with real git repo ──
// These test LiveGitAgent against a temporary git repository.

mod integration {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper to create a temp git repo with an initial commit.
    fn setup_test_repo() -> (TempDir, git2::Repository) {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Configure author for commits
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test User").unwrap();
            config.set_str("user.email", "test@example.com").unwrap();
        }

        // Create initial file and commit
        fs::write(dir.path().join("README.md"), "# Test Project\n").unwrap();

        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("README.md")).unwrap();
            index.write().unwrap();

            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let sig = repo.signature().unwrap();

            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        (dir, repo)
    }

    use super::super::LiveGitAgent;

    #[test]
    fn live_agent_detects_repo() {
        let (dir, _repo) = setup_test_repo();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        assert!(agent.is_repo());
    }

    #[test]
    fn live_agent_returns_repo_root() {
        let (dir, _repo) = setup_test_repo();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let root = agent.repo_root().unwrap();
        // Canonicalize both to handle macOS /var -> /private/var symlink
        let expected = dir.path().canonicalize().unwrap();
        let actual = root.canonicalize().unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn live_agent_detects_branch() {
        let (dir, _repo) = setup_test_repo();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        // Default branch after init is usually "main" or "master"
        let branch = agent.current_branch().unwrap();
        assert!(branch.is_some());
    }

    #[test]
    fn live_agent_detects_unstaged_changes() {
        let (dir, _repo) = setup_test_repo();

        // Modify a tracked file
        fs::write(dir.path().join("README.md"), "# Modified Project\n").unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let files = agent.changed_files_unstaged().unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "README.md");
        assert_eq!(files[0].status, FileStatus::Modified);
    }

    #[test]
    fn live_agent_detects_staged_changes() {
        let (dir, repo) = setup_test_repo();

        // Create and stage a new file
        fs::write(dir.path().join("new_file.rs"), "fn main() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("new_file.rs")).unwrap();
        index.write().unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let files = agent.changed_files_staged().unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "new_file.rs");
        assert_eq!(files[0].status, FileStatus::Added);
    }

    #[test]
    fn live_agent_generates_unstaged_diff() {
        let (dir, _repo) = setup_test_repo();

        // Modify a tracked file
        fs::write(
            dir.path().join("README.md"),
            "# Modified Project\nNew line\n",
        )
        .unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let diffs = agent.diff_unstaged().unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "README.md");
        assert_eq!(diffs[0].status, FileStatus::Modified);
        assert!(!diffs[0].hunks.is_empty());
    }

    #[test]
    fn live_agent_generates_staged_diff() {
        let (dir, repo) = setup_test_repo();

        // Create and stage a new file
        fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("lib.rs")).unwrap();
        index.write().unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let diffs = agent.diff_staged().unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "lib.rs");
        assert_eq!(diffs[0].status, FileStatus::Added);
        assert!(!diffs[0].hunks.is_empty());
    }

    #[test]
    fn live_agent_generates_branch_diff() {
        let (dir, repo) = setup_test_repo();

        // Create a feature branch
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature-branch", &head, false).unwrap();
        repo.set_head("refs/heads/feature-branch").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();

        // Make a change on the feature branch
        fs::write(dir.path().join("feature.rs"), "pub fn new_feature() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("feature.rs")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add feature", &tree, &[&parent])
            .unwrap();

        // Get the default branch name (the initial branch from setup)
        let default_branch = head
            .as_object()
            .peel_to_commit()
            .ok()
            .and_then(|_| {
                repo.branches(Some(git2::BranchType::Local))
                    .ok()?
                    .filter_map(|b| b.ok())
                    .find(|(b, _)| b.name().ok().flatten() != Some("feature-branch"))
                    .and_then(|(b, _)| b.name().ok().flatten().map(String::from))
            })
            .unwrap_or_else(|| "master".to_string());

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let diffs = agent.diff_branch(&default_branch).unwrap();

        assert!(!diffs.is_empty());
        assert!(diffs.iter().any(|d| d.path == "feature.rs"));
    }

    #[test]
    fn live_agent_no_changes_returns_empty() {
        let (dir, _repo) = setup_test_repo();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());

        assert!(agent.changed_files_unstaged().unwrap().is_empty());
        assert!(agent.changed_files_staged().unwrap().is_empty());
        assert!(agent.diff_unstaged().unwrap().is_empty());
        assert!(agent.diff_staged().unwrap().is_empty());
    }

    #[test]
    fn live_agent_detects_deleted_file() {
        let (dir, _repo) = setup_test_repo();

        // Delete the tracked file
        fs::remove_file(dir.path().join("README.md")).unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let files = agent.changed_files_unstaged().unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Deleted);
    }

    // S8: Missing test for detached HEAD
    #[test]
    fn live_agent_detached_head_returns_none() {
        let (dir, repo) = setup_test_repo();
        let head_oid = repo.head().unwrap().target().unwrap();
        repo.set_head_detached(head_oid).unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        assert_eq!(agent.current_branch().unwrap(), None);
    }

    // S9: Missing test for nonexistent branch
    #[test]
    fn live_agent_diff_branch_not_found() {
        let (dir, _repo) = setup_test_repo();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let result = agent.diff_branch("nonexistent-branch");

        assert!(result.is_err());
        match result.unwrap_err() {
            GitError::BranchNotFound(name) => assert_eq!(name, "nonexistent-branch"),
            other => panic!("Expected BranchNotFound, got {other:?}"),
        }
    }

    #[test]
    fn live_agent_stage_files() {
        let (dir, _repo) = setup_test_repo();

        // Create a new file
        fs::write(dir.path().join("new.rs"), "fn new() {}\n").unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        agent.stage_files(&["new.rs"]).unwrap();

        // Verify it's staged
        let staged = agent.changed_files_staged().unwrap();
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].path, "new.rs");
        assert_eq!(staged[0].status, FileStatus::Added);
    }

    #[test]
    fn live_agent_commit() {
        let (dir, _repo) = setup_test_repo();

        // Create and stage a file
        fs::write(dir.path().join("new.rs"), "fn new() {}\n").unwrap();
        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        agent.stage_files(&["new.rs"]).unwrap();

        // Commit
        let oid = agent.commit("Test commit message").unwrap();
        assert!(!oid.is_empty());

        // Verify no staged changes remain
        let staged = agent.changed_files_staged().unwrap();
        assert!(staged.is_empty());
    }

    #[test]
    fn live_agent_commit_empty_index_still_works() {
        let (dir, _repo) = setup_test_repo();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        // Commit with nothing new staged — creates no-op commit
        let result = agent.commit("Empty commit");
        assert!(result.is_ok());
    }

    #[test]
    fn live_agent_stage_multiple_files() {
        let (dir, _repo) = setup_test_repo();

        fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        agent.stage_files(&["a.rs", "b.rs"]).unwrap();

        let staged = agent.changed_files_staged().unwrap();
        assert_eq!(staged.len(), 2);
    }

    #[test]
    fn live_agent_stage_deleted_file() {
        let (dir, _repo) = setup_test_repo();

        fs::remove_file(dir.path().join("README.md")).unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        agent.stage_files(&["README.md"]).unwrap();

        let staged = agent.changed_files_staged().unwrap();
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].status, FileStatus::Deleted);
    }

    // ── diff_all tests ──

    #[test]
    fn diff_all_includes_unstaged_changes() {
        let (dir, _repo) = setup_test_repo();

        // Modify tracked file (unstaged)
        fs::write(dir.path().join("README.md"), "# Changed\n").unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let diffs = agent.diff_all().unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "README.md");
    }

    #[test]
    fn diff_all_includes_staged_changes() {
        let (dir, repo) = setup_test_repo();

        // Create and stage a new file
        fs::write(dir.path().join("new.php"), "<?php echo 1;\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("new.php")).unwrap();
        index.write().unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let diffs = agent.diff_all().unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "new.php");
    }

    #[test]
    fn diff_all_combines_staged_and_unstaged() {
        let (dir, repo) = setup_test_repo();

        // Stage a new file
        fs::write(dir.path().join("staged.php"), "<?php\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.php")).unwrap();
        index.write().unwrap();

        // Also modify a tracked file (unstaged)
        fs::write(dir.path().join("README.md"), "# Changed\n").unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let diffs = agent.diff_all().unwrap();
        assert_eq!(diffs.len(), 2);

        let paths: Vec<&str> = diffs.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"staged.php"));
        assert!(paths.contains(&"README.md"));
    }

    #[test]
    fn diff_all_shows_current_state_after_fix() {
        let (dir, repo) = setup_test_repo();

        // Stage a buggy change
        fs::write(dir.path().join("README.md"), "# Buggy\n").unwrap();
        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("README.md")).unwrap();
            index.write().unwrap();
        }

        // Fix it in working tree (unstaged fix)
        fs::write(dir.path().join("README.md"), "# Fixed\n").unwrap();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let diffs = agent.diff_all().unwrap();

        // Should show ONE diff: HEAD → working tree (the fixed version)
        assert_eq!(diffs.len(), 1);
        assert!(
            diffs[0]
                .hunks
                .iter()
                .any(|h| h.content.contains("+# Fixed"))
        );
        // Should NOT contain the buggy staged version
        assert!(
            !diffs[0]
                .hunks
                .iter()
                .any(|h| h.content.contains("+# Buggy"))
        );
    }

    #[test]
    fn diff_all_empty_when_no_changes() {
        let (dir, _repo) = setup_test_repo();

        let agent = LiveGitAgent::new(dir.path().to_path_buf());
        let diffs = agent.diff_all().unwrap();
        assert!(diffs.is_empty());
    }
}
