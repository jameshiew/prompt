use std::borrow::Cow;
use std::fs::OpenOptions;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::Result;
use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::Ref;
use dashmap::DashMap;
use tokio::fs;

use crate::discovery::DiscoveredFile;
use crate::tokenizer::tokenize;

/// Information collected about a read file.
#[derive(Debug)]
pub(crate) struct FileInfo {
    utf8: String,
    meta: FileMeta,
}

impl FileInfo {
    pub(crate) async fn new(path: PathBuf, excluded: bool) -> anyhow::Result<Self> {
        if excluded {
            return Ok(Self {
                meta: FileMeta {
                    path,
                    binary_detected: false,
                    token_count: 0,
                    excluded,
                },
                utf8: "".to_string(),
            });
        }

        let file = OpenOptions::new().read(true).open(&path)?;
        let buf = BufReader::new(file);
        if (bindet::detect(buf)?).is_some() {
            return Ok(Self {
                meta: FileMeta {
                    path,
                    binary_detected: true,
                    token_count: 0,
                    excluded: true,
                },
                utf8: "".to_string(),
            });
        };

        let buffer = fs::read(&path).await?;
        let text = String::from_utf8_lossy(&buffer);
        let content = annotate_line_numbers(text);
        let tokens = tokenize(&content);
        let meta = FileMeta {
            path,
            binary_detected: false,
            token_count: tokens.len(),
            excluded,
        };

        Ok(Self {
            meta,
            utf8: content,
        })
    }

    pub(crate) fn utf8(&self) -> &str {
        &self.utf8
    }

    pub(crate) fn meta(&self) -> &FileMeta {
        &self.meta
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FileMeta {
    path: PathBuf,
    excluded: bool,
    binary_detected: bool,
    token_count: usize,
}

impl FileMeta {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn binary_detected(&self) -> bool {
        self.binary_detected
    }

    pub(crate) fn token_count(&self) -> usize {
        self.token_count
    }

    pub(crate) fn excluded(&self) -> bool {
        self.excluded
    }
}

#[derive(Default)]
pub struct Files {
    inner: DashMap<PathBuf, FileInfo>,
}

impl Files {
    pub async fn read_from(discovered: Vec<DiscoveredFile>) -> Result<Self> {
        let files = Self::default();
        for disc in discovered {
            let info = FileInfo::new(disc.path.clone(), disc.excluded).await?;
            files.insert(disc.path, info);
        }
        Ok(files)
    }

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

    pub(crate) fn excluded(&self) -> Vec<PathBuf> {
        self.inner
            .iter()
            .filter_map(|entry| {
                let (_, info) = entry.pair();
                if info.meta().excluded() {
                    Some(info.meta().path().to_owned())
                } else {
                    None
                }
            })
            .collect()
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

pub fn strip_dot_prefix(path: &Path) -> &Path {
    path.strip_prefix(".").unwrap_or(path)
}
