use std::borrow::Cow;
use std::io::Read;
use std::path::{Path, PathBuf};

use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::Ref;
use dashmap::DashMap;
use ignore::{WalkParallel, WalkState};
use tiktoken_rs::o200k_base_singleton;

/// Information collected about a read file.
#[derive(Debug)]
pub(crate) struct FileInfo {
    utf8: String,
    meta: FileMeta,
}

impl FileInfo {
    pub(crate) fn new(utf8: String) -> anyhow::Result<Self> {
        // TODO: binary detection
        let tokens = {
            let bpe = o200k_base_singleton();
            let bpe = bpe.lock();
            bpe.encode_with_special_tokens(&utf8)
        };
        let meta = FileMeta {
            token_count: tokens.len(),
        };

        Ok(Self { meta, utf8 })
    }

    pub(crate) fn utf8(&self) -> &str {
        &self.utf8
    }

    pub(crate) fn meta(&self) -> &FileMeta {
        &self.meta
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FileMeta {
    token_count: usize,
}

impl FileMeta {
    pub(crate) fn token_count(&self) -> usize {
        self.token_count
    }
}

#[derive(Default)]
pub(crate) struct Files {
    inner: DashMap<PathBuf, FileInfo>,
}

impl Files {
    fn insert(&self, path: PathBuf, info: FileInfo) {
        self.inner.insert(path, info);
    }

    pub(crate) fn remove(&self, path: &Path) -> Option<FileInfo> {
        self.inner.remove(path).map(|(_, info)| info)
    }

    pub(crate) fn get(&self, path: &Path) -> Option<Ref<PathBuf, FileInfo>> {
        self.inner.get(path)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = RefMulti<PathBuf, FileInfo>> {
        self.inner.iter()
    }
}

impl From<WalkParallel> for Files {
    fn from(walk: WalkParallel) -> Self {
        let files = Files {
            inner: DashMap::new(),
        };
        walk.run(|| {
            Box::new(|result| {
                match result {
                    Ok(dir_entry) => {
                        let path = dir_entry.path();
                        if path.is_dir() || path.is_symlink() {
                            return WalkState::Continue;
                        }

                        let buffer = read_file_sync(path).expect("should be able to read file");
                        let text = String::from_utf8_lossy(&buffer);
                        let content = annotate_line_numbers(text);
                        let info =
                            FileInfo::new(content).expect("should be able to create file info");
                        files.insert(path.to_path_buf(), info);
                    }
                    Err(err) => {
                        panic!("Error reading file: {}", err);
                    }
                }
                WalkState::Continue
            })
        });
        files
    }
}

fn annotate_line_numbers(text: Cow<str>) -> String {
    let line_count = text.lines().count();
    if line_count == 0 {
        return "".to_string();
    }

    let digits = ((line_count as f64).log10().floor() as usize) + 1;
    let width = digits;

    let mut numbered = String::new();
    for (i, line) in text.lines().enumerate() {
        let line_num = i + 1;
        // Right-align the line number within the given width
        numbered.push_str(&format!("{:>width$} {}\n", line_num, line, width = width));
    }

    numbered
}

pub fn read_file_sync(path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}
