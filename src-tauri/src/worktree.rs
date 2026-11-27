//! A ticket's worktree, made the way Tracket's Start working makes it (tracket/src-tauri/src/workspace.rs), so both
//! apps use the same folder for a ticket: `<repo>/.claude/worktrees/<KEY>`, on a branch named exactly `<KEY>`, started
//! from origin's default branch. A launch step with its own ticket runs there.
//!
//! Git runs as a child process (never through a shell), with prompts turned off so a missing credential fails
//! instead of hanging.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.env("GIT_TERMINAL_PROMPT", "0").arg("-C").arg(dir).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().map_err(|e| format!("Couldn't run git ({e}). Is it installed and on the PATH?"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if message.is_empty() { format!("git {} failed", args.first().unwrap_or(&"")) } else { message })
    }
}

/// The same place, whatever the case, separators or `\\?\` prefix (as Tracket compares folders).
fn same_place(a: &Path, b: &Path) -> bool {
    let key = |path: &Path| {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let text = resolved.to_string_lossy().to_string();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text).replace('/', "\\");
        text.trim_end_matches('\\').to_lowercase()
    };
    key(a) == key(b)
}

/// Worktrees git knows about: their folder and branch, out of `git worktree list --porcelain`.
fn registered(root: &Path) -> Result<Vec<(PathBuf, Option<String>)>, String> {
    let listing = git(root, &["worktree", "list", "--porcelain"])?;
    let mut found = Vec::new();
    for block in listing.replace("\r\n", "\n").split("\n\n") {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_string());
            }
        }
        if let Some(path) = path {
            found.push((path, branch));
        }
    }
    Ok(found)
}

/// The ticket `key`'s worktree in the repository of `from` (another ticket's worktree), made when missing. The
/// folder returned is the same place inside it as `from` is inside its own (a monorepo's package). A worktree git
/// already has for the ticket is used as it is; a folder in the way that git doesn't know is left for Tracket's
/// Start working to sort out.
pub fn ticket_worktree(from: &Path, key: &str) -> Result<String, String> {
    let common = git(from, &["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .map_err(|_| format!("{} isn't in a git repository", from.display()))?;
    let common = PathBuf::from(common);
    let root: PathBuf = match (common.file_name(), common.parent()) {
        (Some(name), Some(parent)) if name == ".git" => parent.components().collect(),
        _ => return Err("Bare repositories aren't supported".into()),
    };
    // Where `from` is inside its working tree: "" at the top, "packages/api" in a monorepo
    let subpath = git(from, &["rev-parse", "--show-prefix"])?.trim_end_matches('/').to_string();
    let inside = |worktree: &Path| {
        let folder = worktree.join(&subpath);
        let folder = if !subpath.is_empty() && folder.is_dir() { folder } else { worktree.to_path_buf() };
        folder.to_string_lossy().to_string()
    };

    let usual = root.join(".claude").join("worktrees").join(key);
    let known = registered(&root)?;
    if known.iter().any(|(path, _)| same_place(path, &usual)) {
        return Ok(inside(&usual));
    }
    // The ticket's branch is already checked out somewhere: that is its worktree
    if let Some((path, _)) = known.iter().find(|(_, branch)| branch.as_deref() == Some(key)) {
        return Ok(inside(path));
    }
    if usual.exists() {
        return Err(format!(
            "{} is in the way, and git doesn't know it as a worktree: open the ticket {key} in Tracket and use Start working once",
            usual.display()
        ));
    }

    // origin/HEAD says which branch is the default one; then the usual names
    let base = git(&root, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .ok()
        .or_else(|| {
            ["origin/main", "origin/master"]
                .into_iter()
                .find_map(|c| git(&root, &["rev-parse", "--verify", "--quiet", c]).ok().map(|_| c.to_string()))
        })
        .ok_or("Couldn't find the default branch of origin. Try `git remote set-head origin -a` in that repository")?;
    // Offline is fine: what origin had last time is used
    let _ = git(&root, &["fetch", "origin"]);
    let location = usual.to_string_lossy().to_string();
    let branch_exists = git(&root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{key}")]).is_ok();
    if branch_exists {
        git(&root, &["worktree", "add", &location, key])?;
    } else {
        // Not tracking the default branch: the ticket's branch gets pushed under its own name
        git(&root, &["worktree", "add", "--no-track", "-b", key, &location, &base])?;
    }
    Ok(inside(&usual))
}

/// A clone of a one-commit repository, so it has an origin with a default branch.
#[cfg(test)]
pub fn cloned_repo(dir: &Path) -> PathBuf {
        let origin = dir.join("origin");
        std::fs::create_dir_all(origin.join("packages").join("api")).unwrap();
        std::fs::write(origin.join("packages").join("api").join("a.txt"), "a").unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["add", "."],
            vec!["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "one"],
        ] {
            git(&origin, &args).unwrap();
        }
        git(dir, &["clone", "-q", &origin.to_string_lossy(), "repo"]).unwrap();
        dir.join("repo")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_the_tickets_worktree_where_tracket_does_and_reuses_it() {
        let dir = tempfile::tempdir().unwrap();
        let repo = cloned_repo(dir.path());
        let folder = ticket_worktree(&repo.join("packages").join("api"), "SIS-301").unwrap();
        let usual = repo.join(".claude").join("worktrees").join("SIS-301");
        assert!(same_place(Path::new(&folder), &usual.join("packages").join("api")));
        assert_eq!(git(&usual, &["branch", "--show-current"]).unwrap(), "SIS-301");

        // Again, and from another ticket's worktree: the same folder
        assert!(same_place(Path::new(&ticket_worktree(&repo, "SIS-301").unwrap()), &usual));
        let other = ticket_worktree(&usual, "SIS-302").unwrap();
        assert!(same_place(Path::new(&other), &repo.join(".claude").join("worktrees").join("SIS-302")));

        std::fs::create_dir_all(repo.join(".claude").join("worktrees").join("SIS-303")).unwrap();
        assert!(ticket_worktree(&repo, "SIS-303").unwrap_err().contains("Start working"));
    }
}
