use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use dashmap::DashSet;
use home::home_dir;
use ignore::gitignore::Gitignore;
use ignore::{Match as IgnoreMatch, WalkBuilder, WalkState};
use tracing::warn;

use crate::files::strip_dot_prefix;

const PROMPT_HOME_OVERRIDE_ENV: &str = "PROMPT_HOME_DIR";

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
    no_gitignore: bool,
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
    let mut promptignore_roots = Vec::with_capacity(match_bases.len());
    for base in &match_bases {
        if let Ok(canonical) = std::fs::canonicalize(base) {
            if let Some(root) = promptignore_root(&canonical) {
                promptignore_roots.push(root);
            }
            canonical_bases.push(canonical);
        }
    }
    match_bases.extend(canonical_bases);
    let match_bases = Arc::new(match_bases);
    let promptignore_roots = Arc::new(promptignore_roots);
    walker.hidden(false);
    // use thread heuristic from  https://github.com/BurntSushi/ripgrep/issues/2854
    walker.threads(
        std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(12),
    );
    if no_gitignore {
        walker.git_ignore(false);
        walker.git_global(false);
        walker.git_exclude(false);
    }
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
    apply_promptignore(&mut discovered, &promptignore_roots);
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

fn canonicalize_for_promptignore(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn promptignore_root(path: &Path) -> Option<PathBuf> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.is_dir() {
        Some(path.to_path_buf())
    } else {
        path.parent().map(Path::to_path_buf)
    }
}

fn apply_promptignore(discovered: &mut [DiscoveredFile], roots: &[PathBuf]) {
    let mut matcher = PromptignoreMatcher::new();
    for entry in discovered {
        let absolute_path = canonicalize_for_promptignore(&entry.path);
        let root = find_root_for_path(&absolute_path, roots);
        if matcher.matches(&absolute_path, root.map(|r| r.as_path())) {
            entry.excluded = true;
        }
    }
}

fn find_root_for_path<'a>(path: &Path, roots: &'a [PathBuf]) -> Option<&'a PathBuf> {
    roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
}

struct PromptignoreMatcher {
    directory_cache: HashMap<PathBuf, Option<Gitignore>>,
    global: Option<Gitignore>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptignoreDecision {
    None,
    Ignore,
    Whitelist,
}

impl PromptignoreDecision {
    fn from_match(mat: IgnoreMatch<&ignore::gitignore::Glob>) -> Self {
        if mat.is_ignore() {
            Self::Ignore
        } else if mat.is_whitelist() {
            Self::Whitelist
        } else {
            Self::None
        }
    }

    const fn is_ignore(self) -> bool {
        matches!(self, Self::Ignore)
    }
}

impl PromptignoreMatcher {
    fn new() -> Self {
        Self {
            directory_cache: HashMap::new(),
            global: load_global_promptignore(),
        }
    }

    fn matches(&mut self, path: &Path, root: Option<&Path>) -> bool {
        let is_dir = false;
        let mut decision = PromptignoreDecision::from_match(self.global_match(path, is_dir));
        if let Some(root) = root {
            for dir in directory_chain_within(path, root) {
                if let Some(matcher) = self.matcher_for_dir(&dir) {
                    let mat = matcher.matched_path_or_any_parents(path, is_dir);
                    if !mat.is_none() {
                        decision = PromptignoreDecision::from_match(mat);
                    }
                }
            }
        }
        decision.is_ignore()
    }

    fn global_match(&self, path: &Path, is_dir: bool) -> IgnoreMatch<&ignore::gitignore::Glob> {
        self.global
            .as_ref()
            .filter(|matcher| path.starts_with(matcher.path()))
            .map(|matcher| matcher.matched_path_or_any_parents(path, is_dir))
            .unwrap_or(IgnoreMatch::None)
    }

    fn matcher_for_dir(&mut self, dir: &Path) -> Option<Gitignore> {
        if !self.directory_cache.contains_key(dir) {
            let matcher = load_promptignore_from_dir(dir);
            self.directory_cache.insert(dir.to_path_buf(), matcher);
        }
        self.directory_cache
            .get(dir)
            .and_then(|matcher| matcher.clone())
    }
}

fn directory_chain_within(path: &Path, root: &Path) -> Vec<PathBuf> {
    let mut chain = Vec::new();
    let mut current = path.parent();
    while let Some(dir) = current {
        if !dir.starts_with(root) {
            break;
        }
        chain.push(dir.to_path_buf());
        if dir == root {
            break;
        }
        current = dir.parent();
    }
    chain.reverse();
    chain
}

fn load_promptignore_from_dir(dir: &Path) -> Option<Gitignore> {
    let promptignore = dir.join(".promptignore");
    if !promptignore.exists() {
        return None;
    }
    let (matcher, err) = Gitignore::new(&promptignore);
    if let Some(err) = err {
        warn!("Failed to parse {}: {err}", promptignore.display());
    }
    if matcher.is_empty() {
        None
    } else {
        Some(matcher)
    }
}

fn load_global_promptignore() -> Option<Gitignore> {
    let home = prompt_home_dir()?;
    let promptignore = home.join(".promptignore");
    if !promptignore.exists() {
        return None;
    }
    let (matcher, err) = Gitignore::new(&promptignore);
    if let Some(err) = err {
        warn!("Failed to parse global {}: {err}", promptignore.display());
    }
    if matcher.is_empty() {
        None
    } else {
        Some(matcher)
    }
}

fn prompt_home_dir() -> Option<PathBuf> {
    let path = if let Some(override_dir) = std::env::var_os(PROMPT_HOME_OVERRIDE_ENV) {
        PathBuf::from(override_dir)
    } else {
        home_dir()?
    };
    path.canonicalize().map(Some).unwrap_or(Some(path))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::Result;

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

    #[derive(Default)]
    struct EnvOverride {
        key: &'static str,
    }

    impl EnvOverride {
        fn set_path(key: &'static str, value: &Path) -> Self {
            unsafe { std::env::set_var(key, value) };
            Self { key }
        }
    }

    impl Drop for EnvOverride {
        fn drop(&mut self) {
            unsafe { std::env::remove_var(self.key) };
        }
    }

    #[test]
    fn excludes_apply_to_absolute_paths() -> Result<()> {
        let temp = TempDir::new();
        fs::create_dir_all(temp.path.join("target"))?;
        fs::write(temp.path.join("target/excluded.txt"), b"exclude me")?;
        fs::write(temp.path.join("keep.txt"), b"keep me")?;

        let pattern = glob::Pattern::new("target/**").expect("valid glob pattern");
        let discovered = discover(temp.path.clone(), vec![], vec![pattern], false)?;

        let excluded_entry = discovered
            .iter()
            .find(|entry| entry.path.ends_with("target/excluded.txt"))
            .expect("expected excluded file in discovery results");
        assert!(excluded_entry.excluded, "absolute-path glob did not match");

        Ok(())
    }

    #[test]
    fn gitignored_files_are_skipped_by_default() -> Result<()> {
        let temp = TempDir::new();
        fs::create_dir_all(&temp.path)?;
        fs::create_dir_all(temp.path.join(".git"))?;
        fs::write(temp.path.join(".gitignore"), b"ignored.txt\n")?;
        let ignored = temp.path.join("ignored.txt");
        fs::write(&ignored, b"skip me")?;

        let discovered = discover(temp.path.clone(), vec![], vec![], false)?;
        assert!(discovered.iter().all(|entry| entry.path != ignored));

        Ok(())
    }

    #[test]
    fn gitignored_files_can_be_included() -> Result<()> {
        let temp = TempDir::new();
        fs::create_dir_all(&temp.path)?;
        fs::create_dir_all(temp.path.join(".git"))?;
        fs::write(temp.path.join(".gitignore"), b"ignored.txt\n")?;
        let ignored = temp.path.join("ignored.txt");
        fs::write(&ignored, b"include me")?;

        let discovered = discover(temp.path.clone(), vec![], vec![], true)?;
        assert!(discovered.iter().any(|entry| entry.path == ignored));

        Ok(())
    }

    #[test]
    fn promptignore_marks_files_but_keeps_them_visible() -> Result<()> {
        let temp = TempDir::new();
        fs::create_dir_all(&temp.path)?;
        fs::write(temp.path.join(".promptignore"), b"skip.me\n")?;
        let skip = temp.path.join("skip.me");
        let keep = temp.path.join("keep.me");
        fs::write(&skip, b"skip")?;
        fs::write(&keep, b"keep")?;

        let discovered = discover(temp.path.clone(), vec![], vec![], false)?;

        let skip_entry = discovered
            .iter()
            .find(|entry| entry.path == skip)
            .expect("skip.me should be discovered");
        assert!(
            skip_entry.excluded,
            "promptignore file should mark skip.me excluded"
        );

        let keep_entry = discovered
            .iter()
            .find(|entry| entry.path == keep)
            .expect("keep.me should be discovered");
        assert!(!keep_entry.excluded);

        Ok(())
    }

    #[test]
    fn promptignore_whitelist_overrides_parent_rule() -> Result<()> {
        let temp = TempDir::new();
        fs::create_dir_all(temp.path.join("logs"))?;
        fs::write(temp.path.join(".promptignore"), b"logs/\n")?;
        fs::write(temp.path.join("logs/.promptignore"), b"!keep.log\n")?;
        let ignored = temp.path.join("logs/ignored.log");
        let keep = temp.path.join("logs/keep.log");
        fs::write(&ignored, b"drop")?;
        fs::write(&keep, b"keep")?;

        let discovered = discover(temp.path.clone(), vec![], vec![], false)?;
        let ignored_entry = discovered
            .iter()
            .find(|entry| entry.path == ignored)
            .expect("ignored.log should be present");
        assert!(ignored_entry.excluded);
        let keep_entry = discovered
            .iter()
            .find(|entry| entry.path == keep)
            .expect("keep.log should be present");
        assert!(
            !keep_entry.excluded,
            "nested whitelist should re-include keep.log"
        );

        Ok(())
    }

    #[test]
    fn global_promptignore_applies_when_overridden_home_matches() -> Result<()> {
        let temp_home = TempDir::new();
        fs::create_dir_all(&temp_home.path)?;
        fs::write(temp_home.path.join(".promptignore"), b"*.bin\n")?;
        let project = temp_home.path.join("project");
        fs::create_dir_all(&project)?;
        let binary = project.join("data.bin");
        let text = project.join("notes.txt");
        fs::write(&binary, b"bin")?;
        fs::write(&text, b"text")?;

        let _guard = EnvOverride::set_path(PROMPT_HOME_OVERRIDE_ENV, &temp_home.path);
        let discovered = discover(project, vec![], vec![], false)?;

        let binary_entry = discovered
            .iter()
            .find(|entry| entry.path == binary)
            .expect("data.bin present");
        assert!(
            binary_entry.excluded,
            "global promptignore should exclude *.bin"
        );
        let text_entry = discovered
            .iter()
            .find(|entry| entry.path == text)
            .expect("notes.txt present");
        assert!(!text_entry.excluded);

        Ok(())
    }
}
