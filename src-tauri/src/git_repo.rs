//! Single source of truth for "is this path a git repository?" detection.
//!
//! The check is deliberately strict: the exact path must contain a `.git`
//! entry (directory for regular repos, file for linked worktrees and
//! submodules). We do **not** walk up to ancestors.
//!
//! Rationale: codeg scopes every workspace-facing feature (file tree
//! watcher, git changes panel, log panel) to the directory the user opens.
//! If one code path walks up and another doesn't, the UI falls into a
//! "schizophrenic" state where some panels see a repo and others don't.
//! Keeping the primitive strict forces every consumer onto the same
//! interpretation.
//!
//! Bare repositories are intentionally not supported — they have no working
//! tree, which makes them an unusual target for a workspace-oriented editor.

use std::{
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

use crate::app_error::AppCommandError;

/// Returns true when `path` is the root of a git working tree.
///
/// `.git` may be a directory (normal repo) or a file (worktree/submodule
/// pointer). `Path::exists` treats both as present.
pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Preflight guard for git commands. Short-circuits with a typed error code
/// when the target path is not a git working tree, so callers avoid locale-
/// dependent stderr parsing for the most common "wrong folder" failure.
pub fn ensure_git_repo(path: &str) -> Result<(), AppCommandError> {
    let root = Path::new(path);

    let root_meta = fs::metadata(root).map_err(|err| match err.kind() {
        ErrorKind::NotFound => {
            AppCommandError::not_found(format!("Workspace path does not exist: {path}"))
        }
        ErrorKind::PermissionDenied => {
            AppCommandError::permission_denied(format!("Cannot access workspace path: {path}"))
                .with_detail(err.to_string())
        }
        _ => AppCommandError::io(err)
            .with_detail(format!("Failed to inspect workspace path: {path}")),
    })?;

    if !root_meta.is_dir() {
        return Err(AppCommandError::invalid_input(format!(
            "Workspace path is not a directory: {path}"
        )));
    }

    let git_path = root.join(".git");
    match fs::metadata(&git_path) {
        Ok(_) => Ok(()),
        Err(err) => match err.kind() {
            ErrorKind::NotFound => Err(AppCommandError::not_a_git_repository(format!(
                "Not a Git repository: {path}"
            ))),
            ErrorKind::PermissionDenied => Err(AppCommandError::permission_denied(format!(
                "Cannot access Git metadata: {}",
                git_path.display()
            ))
            .with_detail(err.to_string())),
            _ => Err(AppCommandError::io(err).with_detail(format!(
                "Failed to inspect Git metadata: {}",
                git_path.display()
            ))),
        },
    }
}

/// The main working tree a *linked git worktree* at `dir` belongs to, or `None`
/// when `dir` is not one.
///
/// Read straight off the `.git` pointer file — no `git` subprocess, so this is
/// cheap enough to sit on the folder-open path, and it still answers for a
/// worktree whose repository is momentarily unreadable. Git records the
/// worktree's metadata dir as `gitdir: <main>/.git/worktrees/<name>`; the tail is
/// fixed, so stripping those three components yields `<main>`.
///
/// Requiring that exact tail is the check that separates a linked worktree from
/// a submodule, whose pointer goes to `<super>/.git/modules/<name>` — a
/// submodule is a different repository and must never be filed under its
/// superproject. Layouts that do not end in `.git/worktrees/<name>` (a
/// `--separate-git-dir` repo, a worktree of a bare repo) also return `None`
/// rather than a guess.
///
/// The result is derived lexically (`.`/`..` folded textually, no
/// `canonicalize`): callers compare it against paths the user typed, which are
/// stored verbatim and would not survive symlink resolution.
pub fn linked_worktree_main_root(dir: &Path) -> Option<PathBuf> {
    let dot_git = dir.join(".git");
    // A normal repo's `.git` is a directory; only worktrees and gitlinks use a
    // file. `symlink_metadata` so a symlinked `.git` dir is not mistaken for one.
    if !fs::symlink_metadata(&dot_git).ok()?.file_type().is_file() {
        return None;
    }

    let contents = fs::read_to_string(&dot_git).ok()?;
    let raw = contents
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    // Git writes the pointer relative to the worktree when
    // `worktree.useRelativePaths` is on (`../../../repo/.git/worktrees/wt`).
    let git_dir = PathBuf::from(raw);
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        dir.join(git_dir)
    };
    let git_dir = fold_dot_segments(&git_dir);

    let named = git_dir.parent()?;
    let dot_git_dir = parent_of_component(named, "worktrees")?;
    let root = parent_of_component(dot_git_dir, ".git")?;
    // A pointer of the shape `.git/worktrees/<name>` (no repository above it)
    // folds down to an empty path, which is nothing anyone can open.
    if root.as_os_str().is_empty() {
        return None;
    }
    Some(root.to_path_buf())
}

/// `path`'s parent, but only when `path`'s last component is `name`. Compared
/// case-insensitively: git writes `.git`/`worktrees` in lowercase, but the
/// pointer is plain text on filesystems that do not care about case.
fn parent_of_component<'a>(path: &'a Path, name: &str) -> Option<&'a Path> {
    if !path.file_name()?.to_str()?.eq_ignore_ascii_case(name) {
        return None;
    }
    path.parent()
}

/// Resolve `.` and `..` textually, without consulting the filesystem. A leading
/// `..` that has nothing above it to cancel against is kept, so a relative
/// pointer stays relative rather than silently becoming an absolute path.
fn fold_dot_segments(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod worktree_root_tests {
    use super::*;

    /// Resolve a `.git` pointer holding `contents`, written into a throwaway
    /// directory. The repository it points at never has to exist — the
    /// derivation is textual.
    fn resolved(contents: &str) -> Option<PathBuf> {
        let dir = tempfile::tempdir().expect("tempdir");
        let wt = dir.path().join("wt");
        fs::create_dir_all(&wt).expect("mkdir");
        fs::write(wt.join(".git"), contents).expect("write .git pointer");
        linked_worktree_main_root(&wt)
    }

    /// Absolute pointers are built from a real temp root: a literal like
    /// `/srv/repo` is rootless-but-not-absolute on Windows, which would make the
    /// assertion platform-dependent rather than the code.
    #[test]
    fn absolute_gitdir_yields_the_main_working_tree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("repo");
        let wt = dir.path().join("wt");
        fs::create_dir_all(&wt).expect("mkdir");
        let pointer = repo.join(".git/worktrees/feature");
        fs::write(wt.join(".git"), format!("gitdir: {}\n", pointer.display()))
            .expect("write .git pointer");

        assert_eq!(linked_worktree_main_root(&wt), Some(repo));
    }

    /// `worktree.useRelativePaths` writes the pointer relative to the worktree
    /// itself, so the `..` hops must be folded against that directory.
    #[test]
    fn relative_gitdir_resolves_against_the_worktree_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("repo");
        let wt = dir.path().join("checkouts/feature");
        fs::create_dir_all(&wt).expect("mkdir");
        fs::write(
            wt.join(".git"),
            "gitdir: ../../repo/.git/worktrees/feature\n",
        )
        .expect("write .git pointer");

        assert_eq!(linked_worktree_main_root(&wt), Some(repo));
    }

    /// The common case for everything the user opens: a plain checkout, whose
    /// `.git` is a directory and which is nobody's worktree.
    #[test]
    fn a_normal_repository_is_not_a_worktree() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join(".git")).expect("mkdir .git");
        assert_eq!(linked_worktree_main_root(dir.path()), None);
    }

    #[test]
    fn a_directory_without_git_metadata_is_not_a_worktree() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(linked_worktree_main_root(dir.path()), None);
    }

    #[test]
    fn a_malformed_pointer_file_is_ignored() {
        for contents in ["", "\n\n", "not a pointer", "gitdir:", "gitdir:   \n"] {
            assert_eq!(resolved(contents), None, "contents: {contents:?}");
        }
    }

    /// A submodule's pointer looks almost identical but lands in
    /// `.git/modules/…`; filing it under the superproject would merge two
    /// unrelated repositories into one Path.
    #[test]
    fn a_submodule_pointer_is_not_a_worktree() {
        assert_eq!(resolved("gitdir: /srv/super/.git/modules/vendor\n"), None);
    }

    /// A `--separate-git-dir` repo points straight at a git dir with no
    /// `worktrees/<name>` tail — there is no main working tree to derive.
    #[test]
    fn a_pointer_without_the_worktrees_tail_is_ignored() {
        assert_eq!(resolved("gitdir: /srv/elsewhere/repo.git\n"), None);
    }

    /// Git tolerates CRLF endings and trailing blanks in the pointer, and a
    /// relative pointer exercises the same trimming without needing an absolute
    /// literal.
    #[test]
    fn trailing_whitespace_and_crlf_are_trimmed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wt = dir.path().join("wt");
        fs::create_dir_all(&wt).expect("mkdir");
        fs::write(
            wt.join(".git"),
            "gitdir: ../repo/.git/worktrees/feature  \r\n",
        )
        .expect("write .git pointer");

        assert_eq!(
            linked_worktree_main_root(&wt),
            Some(dir.path().join("repo"))
        );
    }

    /// The pointer is plain text, and on a case-insensitive filesystem a
    /// hand-edited `.GIT/WORKTREES` still names the same directories.
    #[test]
    fn the_fixed_tail_is_matched_case_insensitively() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wt = dir.path().join("wt");
        fs::create_dir_all(&wt).expect("mkdir");
        fs::write(wt.join(".git"), "gitdir: ../repo/.GIT/WorkTrees/feature\n")
            .expect("write .git pointer");

        assert_eq!(
            linked_worktree_main_root(&wt),
            Some(dir.path().join("repo"))
        );
    }

    /// Windows git writes forward slashes into the pointer while the folder
    /// table holds whatever the user's picker produced. Both spellings must
    /// derive the same root — the caller then normalizes separators and case
    /// again before matching it against a stored path. (Backslashes are only
    /// separators on Windows; elsewhere they are ordinary filename characters.)
    #[test]
    #[cfg(target_os = "windows")]
    fn windows_separators_are_interchangeable() {
        let forward = resolved("gitdir: D:/code/repo/.git/worktrees/feature\n");
        let backward = resolved("gitdir: D:\\code\\repo\\.git\\worktrees\\feature\n");
        assert_eq!(forward, Some(PathBuf::from("D:\\code\\repo")));
        assert_eq!(backward, forward);
    }
}
