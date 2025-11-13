use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use dashmap::DashSet;
use ignore::{WalkBuilder, WalkState};

use crate::files::strip_dot_prefix;

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub excluded: bool,
}

/// Returns a sorted [`Vec`] of [`DiscoveredFile`]s
pub fn discover(
    path: PathBuf,
    extra_paths: Vec<PathBuf>,
    exclude: Vec<glob::Pattern>,
) -> Result<Vec<DiscoveredFile>> {
    // Helper function to create error message for non-existent paths
    let path_not_found_error = |path: &PathBuf| {
        anyhow::anyhow!(
            "Path '{}' does not exist. If you're using a glob pattern like '*.go', \
            note that this tool expects actual file or directory paths. \
            Use the --exclude flag with glob patterns to filter files instead.",
            path.display()
        )
    };

    if !path.exists() {
        return Err(path_not_found_error(&path));
    }

    let mut match_bases = Vec::with_capacity(1 + extra_paths.len());
    match_bases.push(path.clone());

    let mut walker = WalkBuilder::new(path);
    for extra_path in &extra_paths {
        if !extra_path.exists() {
            return Err(path_not_found_error(extra_path));
        }
        walker.add(extra_path);
        match_bases.push(extra_path.clone());
    }

    // Include canonicalized bases to cover situations where walker entries are absolute
    // while the user supplied relative paths (or the other way around).
    let mut canonical_bases = Vec::with_capacity(match_bases.len());
    for base in &match_bases {
        if let Ok(canonical) = std::fs::canonicalize(base) {
            canonical_bases.push(canonical);
        }
    }
    match_bases.extend(canonical_bases);
    let match_bases = Arc::new(match_bases);
    walker.hidden(false);
    // use thread heuristic from  https://github.com/BurntSushi/ripgrep/issues/2854
    walker.threads(
        std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(12),
    );
    walker.add_custom_ignore_filename(".promptignore");
    let walker = walker.build_parallel();

    // TODO: use channel to collect results and return early error
    let discovered = Arc::new(DashSet::new());
    let exclude = Arc::new(exclude);
    walker.run(|| {
        let match_bases = Arc::clone(&match_bases);
        let exclude = Arc::clone(&exclude);
        let discovered = Arc::clone(&discovered);
        Box::new(move |result| match result {
            Ok(dir_entry) => {
                let path = dir_entry.path().to_owned();
                if path.is_dir() {
                    // including '.git' in .promptignore doesn't always reliably work e.g. if only included in the global .promptignore
                    if path.components().any(|c| c.as_os_str() == ".git") {
                        return WalkState::Skip;
                    }
                    return WalkState::Continue;
                }
                if path.is_symlink() {
                    return WalkState::Skip;
                }
                let match_path = relativize_for_match(&path, match_bases.as_slice());
                let stored_path = strip_dot_prefix(&path).to_owned();
                let excluded = exclude
                    .iter()
                    .any(|pattern| pattern.matches_path(&match_path));
                discovered.insert(DiscoveredFile {
                    path: stored_path,
                    excluded,
                });
                WalkState::Continue
            }
            Err(err) => {
                panic!("Error reading file: {err}");
            }
        })
    });
    let discovered = Arc::try_unwrap(discovered).expect("walker should release all refs");
    let mut discovered: Vec<_> = discovered.into_iter().collect();
    discovered.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(discovered)
}

fn relativize_for_match(path: &Path, bases: &[PathBuf]) -> PathBuf {
    for base in bases {
        if let Ok(stripped) = path.strip_prefix(base) {
            return strip_dot_prefix(stripped).to_owned();
        }
    }
    strip_dot_prefix(path).to_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("prompt-test-{unique}"));
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn excludes_apply_to_absolute_paths() -> Result<()> {
        let temp = TempDir::new();
        fs::create_dir_all(temp.path.join("target"))?;
        fs::write(temp.path.join("target/excluded.txt"), b"exclude me")?;
        fs::write(temp.path.join("keep.txt"), b"keep me")?;

        let pattern = glob::Pattern::new("target/**").expect("valid glob pattern");
        let discovered = discover(temp.path.clone(), vec![], vec![pattern])?;

        let excluded_entry = discovered
            .iter()
            .find(|entry| entry.path.ends_with("target/excluded.txt"))
            .expect("expected excluded file in discovery results");
        assert!(excluded_entry.excluded, "absolute-path glob did not match");

        Ok(())
    }
}
