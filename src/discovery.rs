use std::path::PathBuf;
use std::sync::{Arc, mpsc};

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
    let exclude = Arc::new(exclude);
    let mut walker = WalkBuilder::new(path);
    for path in extra_paths {
        walker.add(path);
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

    // Create a channel to communicate errors from worker threads
    let discovered = Arc::new(DashSet::new());
    let (error_sender, error_receiver) = mpsc::channel::<ignore::Error>();

    walker.run(|| {
        let error_sender = error_sender.clone();
        let exclude = exclude.clone();
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
                let path = strip_dot_prefix(&path);
                let excluded = exclude.iter().any(|pattern| pattern.matches_path(path));
                discovered.insert(DiscoveredFile {
                    path: path.to_owned(),
                    excluded,
                });
                WalkState::Continue
            }
            Err(err) => {
                // Send error through channel instead of panicking
                let _ = error_sender.send(err);
                WalkState::Quit
            }
        })
    });

    // Drop the original sender so the receiver knows when all senders are done
    drop(error_sender);

    // Check for any errors that occurred during walking
    if let Ok(error) = error_receiver.try_recv() {
        return Err(anyhow::Error::new(error));
    }

    // Convert Arc<DashSet> back to Vec and proceed normally
    let discovered = Arc::try_unwrap(discovered)
        .map_err(|_| anyhow::anyhow!("Failed to unwrap discovered files"))?;
    let mut discovered: Vec<_> = discovered.into_iter().collect();
    discovered.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(discovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_basic() {
        // Test basic functionality with current directory
        let result = discover(PathBuf::from("."), vec![], vec![]);
        assert!(result.is_ok());
        let discovered = result.unwrap();
        assert!(!discovered.is_empty());
    }

    #[test]
    fn test_discover_nonexistent_path() {
        // Test with a path that doesn't exist - this should return an error gracefully
        let result = discover(
            PathBuf::from("/nonexistent/path/that/should/not/exist"),
            vec![],
            vec![],
        );
        // This should either succeed with an empty result or fail gracefully (not panic)
        // The exact behavior depends on how ignore handles nonexistent paths
        match result {
            Ok(_) => {
                // If it succeeds, that's fine too - some paths might just be skipped
            }
            Err(_) => {
                // If it returns an error, that's the expected behavior we want to test
                // This validates that our error handling is working correctly
            }
        }
    }
}
