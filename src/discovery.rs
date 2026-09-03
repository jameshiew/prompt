use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use dashmap::DashMap;
use home::home_dir;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::{Match as IgnoreMatch, WalkBuilder, WalkState};
use tracing::warn;

use crate::capabilities::{CapabilityRoots, FileAccess, ambient_file_access};
use crate::files::strip_dot_prefix;

const PROMPT_HOME_OVERRIDE_ENV: &str = "PROMPT_HOME_DIR";

#[derive(Debug)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub excluded: bool,
    pub(crate) access: Option<FileAccess>,
}

/// Returns a sorted [`Vec`] of [`DiscoveredFile`]s
pub fn discover(
    path: PathBuf,
    extra_paths: Vec<PathBuf>,
    exclude: Vec<String>,
    no_gitignore: bool,
) -> Result<Vec<DiscoveredFile>> {
    // Helper function to create error message for non-existent paths
    let path_not_found_error = |path: &Path| {
        anyhow::anyhow!(
            "Path '{}' does not exist. If you're using a glob pattern like '*.go', \
            note that this tool expects actual file or directory paths. \
            Use the --exclude flag with glob patterns to filter files instead.",
            path.display()
        )
    };

    let mut match_bases = Vec::with_capacity(1 + extra_paths.len());
    match_bases.push(path.clone());
    match_bases.extend(extra_paths.iter().cloned());

    let mut capability_roots = CapabilityRoots::default();
    for base in &match_bases {
        if let Err(error) = capability_roots.add(base) {
            if error.kind() == std::io::ErrorKind::NotFound {
                return Err(path_not_found_error(base));
            }
            return Err(error).with_context(|| format!("failed to open {}", base.display()));
        }
    }

    let mut walker = WalkBuilder::new(path);
    for extra_path in &extra_paths {
        walker.add(extra_path);
    }

    match_bases.extend(capability_roots.canonical_paths());
    let match_bases = Arc::new(match_bases);
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

    let mut overrides_builder = OverrideBuilder::new("");
    for pattern in &exclude {
        if pattern.ends_with('/') {
            overrides_builder.add(&format!("{pattern}**"))?;
        } else {
            overrides_builder.add(pattern)?;
        }
    }
    let overrides = Arc::new(overrides_builder.build()?);

    let discovered = Arc::new(DashMap::new());
    let walk_error = Arc::new(Mutex::new(None));
    walker.run(|| {
        let match_bases = Arc::clone(&match_bases);
        let overrides = Arc::clone(&overrides);
        let discovered = Arc::clone(&discovered);
        let walk_error = Arc::clone(&walk_error);
        Box::new(move |result| match result {
            Ok(dir_entry) => {
                let Some(file_type) = dir_entry.file_type() else {
                    return WalkState::Continue;
                };
                let path = dir_entry.path().to_owned();
                if file_type.is_dir() {
                    // Always skip Git metadata directories. A global .promptignore does not reliably exclude them.
                    if path
                        .components()
                        .any(|c| c.as_os_str().eq_ignore_ascii_case(".git"))
                    {
                        return WalkState::Skip;
                    }
                    return WalkState::Continue;
                }
                if !file_type.is_file() {
                    return WalkState::Continue;
                }
                let stored_path = strip_dot_prefix(&path).to_owned();
                let excluded = matches_exclude(&path, match_bases.as_slice(), &overrides);
                discovered
                    .entry(stored_path)
                    .and_modify(|stored_excluded| *stored_excluded |= excluded)
                    .or_insert(excluded);
                WalkState::Continue
            }
            Err(err) => {
                let mut guard = walk_error
                    .lock()
                    .expect("walk error mutex should not be poisoned");
                if guard.is_none() {
                    *guard = Some(anyhow::anyhow!("failed to read entry: {err}"));
                }
                WalkState::Quit
            }
        })
    });
    let walk_error = walk_error
        .lock()
        .expect("walk error mutex should not be poisoned")
        .take();
    if let Some(err) = walk_error {
        return Err(err);
    }
    let discovered = Arc::try_unwrap(discovered).expect("walker should release all refs");
    let mut discovered: Vec<_> = discovered
        .into_iter()
        .map(|(path, excluded)| DiscoveredFile {
            path,
            excluded,
            access: None,
        })
        .collect();
    apply_promptignore(&mut discovered, &capability_roots);
    discovered.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(discovered)
}

fn matches_exclude(path: &Path, bases: &[PathBuf], overrides: &Override) -> bool {
    let mut matched_base = false;
    for base in bases {
        if let Ok(stripped) = path.strip_prefix(base) {
            matched_base = true;
            let match_path = if stripped.as_os_str().is_empty()
                && let Some(file_name) = base.file_name()
            {
                Path::new(file_name)
            } else {
                strip_dot_prefix(stripped)
            };
            if overrides.matched(match_path, false).is_whitelist() {
                return true;
            }
        }
    }
    !matched_base
        && overrides
            .matched(strip_dot_prefix(path), false)
            .is_whitelist()
}

fn apply_promptignore(discovered: &mut [DiscoveredFile], capabilities: &CapabilityRoots) {
    let roots = capabilities.promptignore_roots();
    let mut matcher = PromptignoreMatcher::new(capabilities);
    for entry in discovered {
        if entry.excluded {
            continue;
        }
        let (absolute_path, access) = match capabilities.resolve_file(&entry.path) {
            Ok(Some(resolved)) => resolved,
            Ok(None) => {
                warn!(
                    "Cannot resolve {} beneath an input root. The file will be excluded.",
                    entry.path.display()
                );
                entry.excluded = true;
                continue;
            }
            Err(error) => {
                warn!(
                    "Cannot resolve {} beneath its input root: {error}. The file will be excluded.",
                    entry.path.display()
                );
                entry.excluded = true;
                continue;
            }
        };
        let root = find_root_for_path(&absolute_path, &roots);
        if matcher.matches(&absolute_path, root.map(|r| r.as_path())) {
            entry.excluded = true;
            continue;
        }
        entry.access = Some(access);
    }
}

fn find_root_for_path<'a>(path: &Path, roots: &'a [PathBuf]) -> Option<&'a PathBuf> {
    roots
        .iter()
        .filter(|root| path.starts_with(root))
        .min_by_key(|root| root.components().count())
}

struct PromptignoreMatcher<'a> {
    capabilities: &'a CapabilityRoots,
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

impl<'a> PromptignoreMatcher<'a> {
    fn new(capabilities: &'a CapabilityRoots) -> Self {
        Self {
            capabilities,
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
            let matcher = load_promptignore_from_dir(dir, self.capabilities);
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

fn load_promptignore_from_dir(dir: &Path, capabilities: &CapabilityRoots) -> Option<Gitignore> {
    let access = capabilities.access_in_dir(dir, Path::new(".promptignore"))?;
    load_promptignore(access, dir, &dir.join(".promptignore"))
}

fn load_global_promptignore() -> Option<Gitignore> {
    let home = prompt_home_dir()?;
    let (canonical_home, access) = match ambient_file_access(&home, Path::new(".promptignore")) {
        Ok(access) => access,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            warn!(
                "Failed to open global .promptignore directory {}: {error}",
                home.display()
            );
            return None;
        }
    };
    let promptignore = canonical_home.join(".promptignore");
    load_promptignore(access, &canonical_home, &promptignore)
}

fn load_promptignore(access: FileAccess, root: &Path, path: &Path) -> Option<Gitignore> {
    let file = match access.open_following() {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            warn!("Failed to open {}: {error}", path.display());
            return None;
        }
    };

    let mut builder = GitignoreBuilder::new(root);
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index.saturating_add(1);
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                warn!(
                    "Failed to read {} at line {line_number}: {error}",
                    path.display()
                );
                break;
            }
        };
        let line = if index == 0 {
            line.trim_start_matches('\u{feff}')
        } else {
            &line
        };
        if let Err(error) = builder.add_line(Some(path.to_path_buf()), line) {
            warn!(
                "Failed to parse {} at line {line_number}: {error}",
                path.display()
            );
        }
    }

    match builder.build() {
        Ok(matcher) if !matcher.is_empty() => Some(matcher),
        Ok(_) => None,
        Err(error) => {
            warn!("Failed to parse {}: {error}", path.display());
            None
        }
    }
}

fn prompt_home_dir() -> Option<PathBuf> {
    let path = if let Some(override_dir) = std::env::var_os(PROMPT_HOME_OVERRIDE_ENV)
        && !override_dir.is_empty()
    {
        PathBuf::from(override_dir)
    } else {
        home_dir()?
    };
    Some(path)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
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

        fn set(key: &'static str, value: &OsStr) -> Self {
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

        let discovered = discover(temp.path.clone(), vec![], vec!["target/**".into()], false)?;

        let excluded_entry = discovered
            .iter()
            .find(|entry| entry.path.ends_with("target/excluded.txt"))
            .expect("expected excluded file in discovery results");
        assert!(excluded_entry.excluded, "absolute-path glob did not match");

        Ok(())
    }

    #[test]
    fn directory_excludes_apply_to_descendant_files() -> Result<()> {
        let temp = TempDir::new();
        fs::create_dir_all(temp.path.join("out/nested"))?;
        let direct = temp.path.join("out/direct.txt");
        let nested = temp.path.join("out/nested/child.txt");
        let keep = temp.path.join("keep.txt");
        fs::write(&direct, b"exclude me")?;
        fs::write(&nested, b"exclude me too")?;
        fs::write(&keep, b"keep me")?;

        let discovered = discover(temp.path.clone(), vec![], vec!["out/".into()], false)?;

        for excluded in [direct, nested] {
            let entry = discovered
                .iter()
                .find(|entry| entry.path == excluded)
                .expect("descendant file should be discovered");
            assert!(entry.excluded, "directory exclude should match descendants");
        }
        let keep_entry = discovered
            .iter()
            .find(|entry| entry.path == keep)
            .expect("unmatched file should be discovered");
        assert!(!keep_entry.excluded);

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
    fn git_metadata_directories_are_skipped_case_insensitively() -> Result<()> {
        let temp = TempDir::new();
        let metadata_file = temp.path.join(".GIT/config");
        fs::create_dir_all(metadata_file.parent().expect("config should have a parent"))?;
        fs::write(&metadata_file, b"secret")?;

        let discovered = discover(temp.path.clone(), vec![], vec![], true)?;

        assert!(discovered.iter().all(|entry| entry.path != metadata_file));

        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_files_are_not_discovered() -> Result<()> {
        use std::os::unix::net::UnixListener;

        let temp = TempDir::new();
        fs::create_dir_all(&temp.path)?;
        let socket_path = temp.path.join("service.sock");
        let _listener = UnixListener::bind(&socket_path)?;

        let discovered = discover(temp.path.clone(), vec![], vec![], true)?;

        assert!(discovered.iter().all(|entry| entry.path != socket_path));

        Ok(())
    }

    #[test]
    fn exact_file_root_can_be_excluded_by_name() -> Result<()> {
        let temp = TempDir::new();
        fs::create_dir_all(temp.path.join("src"))?;
        let main_rs = temp.path.join("src/main.rs");
        fs::write(&main_rs, b"fn main() {}\n")?;

        let discovered = discover(main_rs.clone(), vec![], vec!["main.rs".into()], false)?;

        let main_entry = discovered
            .iter()
            .find(|entry| entry.path == main_rs)
            .expect("main.rs should be discovered");
        assert!(
            main_entry.excluded,
            "exact file root should match name-based excludes"
        );

        Ok(())
    }

    #[test]
    fn overlapping_roots_merge_to_excluded_when_any_match_excludes() -> Result<()> {
        let temp = TempDir::new();
        fs::create_dir_all(temp.path.join("src"))?;
        let main_rs = temp.path.join("src/main.rs");
        fs::write(&main_rs, b"fn main() {}\n")?;

        let discovered = discover(
            main_rs.clone(),
            vec![temp.path.join("src")],
            vec!["main.rs".into()],
            false,
        )?;

        let entries_for_main = discovered
            .iter()
            .filter(|entry| entry.path == main_rs)
            .collect::<Vec<_>>();
        assert_eq!(
            entries_for_main.len(),
            1,
            "discovery should deduplicate by path"
        );
        assert!(
            entries_for_main[0].excluded,
            "exclusion should win when any discovery path excludes a file"
        );

        Ok(())
    }

    #[test]
    fn overlapping_roots_apply_excludes_for_each_root_order() -> Result<()> {
        let root = PathBuf::from("project");
        let nested_root = root.join("src");
        let main_rs = nested_root.join("nested/main.rs");
        let mut overrides = OverrideBuilder::new("");
        overrides.add("nested/main.rs")?;
        let overrides = overrides.build()?;

        for match_bases in [[root.clone(), nested_root.clone()], [nested_root, root]] {
            assert!(
                matches_exclude(&main_rs, &match_bases, &overrides),
                "exclude should match relative to the nested root"
            );
        }

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
    fn promptignore_fails_closed_when_a_discovered_file_moves() -> Result<()> {
        let temp = TempDir::new();
        let root = temp.path.join("project");
        fs::create_dir_all(temp.path.join("alias"))?;
        fs::create_dir_all(&root)?;
        fs::write(root.join(".promptignore"), b"secret.txt\n")?;
        let secret = temp.path.join("alias/../project/secret.txt");
        fs::write(&secret, b"secret")?;
        let mut capabilities = CapabilityRoots::default();
        capabilities.add(&root)?;
        fs::rename(&secret, root.join("moved.txt"))?;
        let mut discovered = [DiscoveredFile {
            path: secret,
            excluded: false,
            access: None,
        }];

        apply_promptignore(&mut discovered, &capabilities);

        assert!(
            discovered[0].excluded,
            "a canonicalization failure must exclude the file"
        );

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

    fn assert_overlapping_promptignore_roots(parent_first: bool) -> Result<()> {
        let temp = TempDir::new();
        let nested = temp.path.join("src");
        fs::create_dir_all(&nested)?;
        fs::write(temp.path.join(".promptignore"), b"src/\n")?;
        fs::write(nested.join(".promptignore"), b"!keep.txt\n")?;
        let ignored = nested.join("ignored.txt");
        let keep = nested.join("keep.txt");
        fs::write(&ignored, b"drop")?;
        fs::write(&keep, b"keep")?;

        let (path, extra_paths) = if parent_first {
            (temp.path.clone(), vec![nested])
        } else {
            (nested, vec![temp.path.clone()])
        };
        let discovered = discover(path, extra_paths, vec![], false)?;

        let ignored_entry = discovered
            .iter()
            .find(|entry| entry.path == ignored)
            .expect("ignored.txt should be present");
        assert!(
            ignored_entry.excluded,
            "parent rule should exclude ignored.txt"
        );
        let keep_entry = discovered
            .iter()
            .find(|entry| entry.path == keep)
            .expect("keep.txt should be present");
        assert!(
            !keep_entry.excluded,
            "nested whitelist should re-include keep.txt"
        );

        Ok(())
    }

    #[test]
    fn overlapping_promptignore_roots_preserve_rules_parent_first() -> Result<()> {
        assert_overlapping_promptignore_roots(true)
    }

    #[test]
    fn overlapping_promptignore_roots_preserve_rules_nested_first() -> Result<()> {
        assert_overlapping_promptignore_roots(false)
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

    #[test]
    fn empty_home_override_is_unset() {
        let _guard = EnvOverride::set(PROMPT_HOME_OVERRIDE_ENV, OsStr::new(""));

        assert_eq!(prompt_home_dir(), home_dir());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directories_return_an_error() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new();
        let locked = temp.path.join("locked");
        fs::create_dir_all(&locked)?;
        fs::write(temp.path.join("ok.txt"), b"ok")?;
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))?;

        let result = discover(temp.path.clone(), vec![], vec![], false);

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700))?;

        match result {
            Ok(_) => {
                if fs::read_dir(&locked).is_ok() {
                    return Ok(());
                }
                panic!("discover should fail on unreadable directories");
            }
            Err(err) => {
                let message = format!("{err:#}");
                assert!(
                    message.contains("failed to read"),
                    "unexpected error: {message}"
                );
            }
        }

        Ok(())
    }
}
