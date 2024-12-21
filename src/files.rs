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
    pub(crate) utf8: String,
    pub(crate) meta: FileMeta,
}

impl FileInfo {
    pub(crate) async fn new(
        path: PathBuf,
        excluded: bool,
        count_tokens: bool,
    ) -> anyhow::Result<Self> {
        if excluded {
            return Ok(Self {
                meta: FileMeta {
                    path,
                    binary_detected: false,
                    token_count: None,
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
                    token_count: None,
                    excluded: true,
                },
                utf8: "".to_string(),
            });
        };

        let buffer = fs::read(&path).await?;
        let text = String::from_utf8_lossy(&buffer);
        let content = annotate_line_numbers(text);
        let token_count = if count_tokens {
            let tokens = tokenize(&content);
            Some(tokens.len())
        } else {
            None
        };
        let meta = FileMeta {
            path,
            binary_detected: false,
            token_count,
            excluded,
        };

        Ok(Self {
            meta,
            utf8: content,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FileMeta {
    pub(crate) path: PathBuf,
    pub(crate) excluded: bool,
    pub(crate) binary_detected: bool,
    pub(crate) token_count: Option<usize>,
}

#[derive(Default)]
pub struct Files {
    inner: DashMap<PathBuf, FileInfo>,
}

impl Files {
    pub async fn read_from(discovered: Vec<DiscoveredFile>, count_tokens: bool) -> Result<Self> {
        let files = Self::default();
        for disc in discovered {
            let info = FileInfo::new(disc.path.clone(), disc.excluded, count_tokens).await?;
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

    pub(crate) fn get_excluded(&self) -> Vec<PathBuf> {
        self.inner
            .iter()
            .filter_map(|entry| {
                let (_, info) = entry.pair();
                if info.meta.excluded {
                    Some(info.meta.path.to_owned())
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
