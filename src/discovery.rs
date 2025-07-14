use std::path::PathBuf;
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

    let mut walker = WalkBuilder::new(path);
    for extra_path in extra_paths {
        if !extra_path.exists() {
            return Err(path_not_found_error(&extra_path));
        }
        walker.add(extra_path);
    }
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
    let discovered = DashSet::new();
    let exclude = Arc::new(exclude);
    walker.run(|| {
        Box::new(|result| match result {
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
                let path = strip_dot_prefix(&path);
                let excluded = exclude.iter().any(|pattern| pattern.matches_path(path));
                discovered.insert(DiscoveredFile {
                    path: path.to_owned(),
                    excluded,
                });
                WalkState::Continue
            }
            Err(err) => {
                panic!("Error reading file: {err}");
            }
        })
    });
    let mut discovered: Vec<_> = discovered.into_iter().collect();
    discovered.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(discovered)
}
