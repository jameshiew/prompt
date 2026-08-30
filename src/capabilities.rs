use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fmt, io};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, File, OpenOptions};

#[derive(Clone)]
pub struct FileAccess {
    dir: Arc<Dir>,
    path: PathBuf,
}

impl FileAccess {
    pub(super) fn open(&self) -> io::Result<File> {
        self.open_with_follow(FollowSymlinks::No)
    }

    pub(super) fn open_following(&self) -> io::Result<File> {
        self.open_with_follow(FollowSymlinks::Yes)
    }

    fn open_with_follow(&self, follow: FollowSymlinks) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true).follow(follow).nonblock(true);

        let file = self.dir.open_with(&self.path, &options)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path is not a regular file",
            ));
        }
        Ok(file)
    }
}

impl fmt::Debug for FileAccess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileAccess")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

enum Selection {
    Directory,
    File(PathBuf),
}

struct CapabilityRoot {
    aliases: Vec<PathBuf>,
    base: PathBuf,
    dir: Arc<Dir>,
    selection: Selection,
}

#[derive(Default)]
pub struct CapabilityRoots {
    roots: Vec<CapabilityRoot>,
}

impl CapabilityRoots {
    pub(super) fn add(&mut self, path: &Path) -> io::Result<()> {
        let canonical = path.canonicalize()?;
        let metadata = canonical.metadata()?;
        let (base, selected_path, selection) = if metadata.is_dir() {
            (canonical.clone(), canonical, Selection::Directory)
        } else {
            let base = canonical
                .parent()
                .ok_or_else(|| io::Error::other("file path has no parent directory"))?
                .to_path_buf();
            let file_name = canonical
                .file_name()
                .ok_or_else(|| io::Error::other("file path has no file name"))?
                .into();
            (base, canonical, Selection::File(file_name))
        };
        let dir = Dir::open_ambient_dir(&base, ambient_authority())?;
        let mut aliases = vec![path.to_path_buf(), selected_path];
        if let Ok(without_dot) = path.strip_prefix(".") {
            aliases.push(without_dot.to_path_buf());
        }
        aliases.sort();
        aliases.dedup();
        self.roots.push(CapabilityRoot {
            aliases,
            base,
            dir: Arc::new(dir),
            selection,
        });
        Ok(())
    }

    pub(super) fn resolve_file(&self, path: &Path) -> io::Result<Option<(PathBuf, FileAccess)>> {
        let Some((_, root, relative)) = self
            .roots
            .iter()
            .filter_map(|root| {
                let relative = root.relative_path(path)?;
                let exact_file = matches!(root.selection, Selection::File(_));
                Some(((exact_file, root.base.components().count()), root, relative))
            })
            .max_by_key(|(specificity, _, _)| *specificity)
        else {
            return Ok(None);
        };

        let canonical_relative = root.dir.canonicalize(&relative)?;
        let absolute = root.base.join(canonical_relative);
        Ok(Some((
            absolute,
            FileAccess {
                dir: Arc::clone(&root.dir),
                path: relative,
            },
        )))
    }

    pub(super) fn access_in_dir(&self, dir: &Path, file: &Path) -> Option<FileAccess> {
        self.roots
            .iter()
            .filter_map(|root| {
                let relative_dir = dir.strip_prefix(&root.base).ok()?;
                let allowed = match &root.selection {
                    Selection::Directory => true,
                    Selection::File(_) => relative_dir.as_os_str().is_empty(),
                };
                allowed.then_some((root.base.components().count(), root, relative_dir))
            })
            .max_by_key(|(depth, _, _)| *depth)
            .map(|(_, root, relative_dir)| FileAccess {
                dir: Arc::clone(&root.dir),
                path: relative_dir.join(file),
            })
    }

    pub(super) fn promptignore_roots(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|root| root.base.clone()).collect()
    }

    pub(super) fn canonical_paths(&self) -> Vec<PathBuf> {
        self.roots
            .iter()
            .map(CapabilityRoot::selected_path)
            .collect()
    }
}

impl CapabilityRoot {
    fn relative_path(&self, path: &Path) -> Option<PathBuf> {
        match &self.selection {
            Selection::Directory => self
                .aliases
                .iter()
                .filter_map(|alias| path.strip_prefix(alias).ok())
                .min_by_key(|relative| relative.components().count())
                .map(Path::to_path_buf),
            Selection::File(file) => self
                .aliases
                .iter()
                .any(|alias| path == alias)
                .then(|| file.clone()),
        }
    }

    fn selected_path(&self) -> PathBuf {
        match &self.selection {
            Selection::Directory => self.base.clone(),
            Selection::File(file) => self.base.join(file),
        }
    }
}

pub fn ambient_file_access(dir: &Path, file: &Path) -> io::Result<(PathBuf, FileAccess)> {
    let canonical = dir.canonicalize()?;
    let dir = Dir::open_ambient_dir(&canonical, ambient_authority())?;
    Ok((
        canonical,
        FileAccess {
            dir: Arc::new(dir),
            path: file.to_path_buf(),
        },
    ))
}
