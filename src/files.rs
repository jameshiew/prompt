use std::borrow::Cow;
use std::fs::OpenOptions;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::Result;
use dashmap::DashMap;
use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::Ref;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::discovery::DiscoveredFile;
use crate::tokenizer::tokenize;

const BINARY_DETECTION_BYTES: usize = 8 * 1024;
const TEXTUAL_MIME_PREFIX: &str = "text/";

fn is_probably_binary(sample: &[u8]) -> bool {
    if let Some(kind) = infer::get(sample) {
        let mime = kind.mime_type();
        if mime.starts_with(TEXTUAL_MIME_PREFIX) || is_textual_mime(mime) {
            return false;
        }

        return kind.matcher_type() != infer::MatcherType::Text;
    }

    // Fallback heuristic: treat presence of null bytes as binary.
    sample.contains(&0)
}

fn is_textual_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/json"
            | "application/xml"
            | "application/javascript"
            | "application/graphql"
            | "application/sql"
    )
}

/// Information collected about a read file.
#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub utf8: Option<String>,
    pub meta: FileMeta,
}

impl FileInfo {
    pub async fn new(path: PathBuf, excluded: bool, count_tokens: bool) -> anyhow::Result<Self> {
        if excluded {
            return Ok(Self {
                meta: FileMeta {
                    path,
                    read_status: ReadStatus::ExcludedExplicitly,
                },
                utf8: None,
            });
        }

        let file = OpenOptions::new().read(true).open(&path)?;
        let mut reader = BufReader::new(file);
        let mut sample = [0u8; BINARY_DETECTION_BYTES];
        let read = reader.read(&mut sample)?;
        if is_probably_binary(&sample[..read]) {
            return Ok(Self {
                meta: FileMeta {
                    path,
                    read_status: ReadStatus::ExcludedBinaryDetected,
                },
                utf8: None,
            });
        };

        let buffer = fs::read(&path).await?;
        let text = String::from_utf8_lossy(&buffer);
        let content = annotate_line_numbers(text);
        let meta = if count_tokens {
            let tokens = tokenize(&content);
            FileMeta {
                path,
                read_status: ReadStatus::TokenCounted(tokens.len()),
            }
        } else {
            FileMeta {
                path,
                read_status: ReadStatus::Read,
            }
        };

        Ok(Self {
            meta,
            utf8: Some(content),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub path: PathBuf,
    pub read_status: ReadStatus,
}

impl FileMeta {
    pub const fn is_excluded(&self) -> bool {
        matches!(
            self.read_status,
            ReadStatus::ExcludedExplicitly | ReadStatus::ExcludedBinaryDetected
        )
    }

    pub const fn token_count_or_zero(&self) -> usize {
        let ReadStatus::TokenCounted(token_count) = &self.read_status else {
            return 0;
        };
        *token_count
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReadStatus {
    ExcludedExplicitly,
    ExcludedBinaryDetected,
    Read,
    TokenCounted(usize),
}

#[derive(Default)]
pub struct Files {
    inner: DashMap<PathBuf, FileInfo>,
}

impl Serialize for Files {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        let mut paths = self
            .inner
            .iter()
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>();
        paths.sort();

        for path in paths {
            let file = self
                .get(&path)
                .expect("path collected from map should still exist");
            map.serialize_entry(&path, file.value())?;
        }
        map.end()
    }
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

    pub fn remove(&self, path: &Path) -> Option<FileInfo> {
        self.inner.remove(path).map(|(_, info)| info)
    }

    pub fn get(&self, path: &Path) -> Option<Ref<'_, PathBuf, FileInfo>> {
        self.inner.get(path)
    }

    pub fn iter(&self) -> impl Iterator<Item = RefMulti<'_, PathBuf, FileInfo>> {
        self.inner.iter()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn get_excluded(&self) -> Vec<PathBuf> {
        self.inner
            .iter()
            .filter_map(|entry| {
                let (_, info) = entry.pair();
                if info.meta.is_excluded() {
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
        numbered.push_str(&format!("{line_num:>width$} {line}\n"));
    }

    numbered
}

pub fn strip_dot_prefix(path: &Path) -> &Path {
    path.strip_prefix(".").unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_file(path: &str) -> (PathBuf, FileInfo) {
        let path = PathBuf::from(path);
        let info = FileInfo {
            utf8: Some("1 test".to_string()),
            meta: FileMeta {
                path: path.clone(),
                read_status: ReadStatus::Read,
            },
        };

        (path, info)
    }

    #[test]
    fn serialized_structured_output_is_stable_and_sorted() {
        let files = Files::default();
        for path in ["zeta.rs", "alpha.rs", "middle.rs"] {
            let (path, info) = build_file(path);
            files.insert(path, info);
        }

        let json_runs = (0..5)
            .map(|_| serde_json::to_string(&files).expect("json serialization should work"))
            .collect::<Vec<_>>();
        let yaml_runs = (0..5)
            .map(|_| serde_norway::to_string(&files).expect("yaml serialization should work"))
            .collect::<Vec<_>>();

        assert!(json_runs.windows(2).all(|pair| pair[0] == pair[1]));
        assert!(yaml_runs.windows(2).all(|pair| pair[0] == pair[1]));

        let json = &json_runs[0];
        let alpha_pos = json
            .find("\"alpha.rs\"")
            .expect("alpha.rs should be in serialized output");
        let middle_pos = json
            .find("\"middle.rs\"")
            .expect("middle.rs should be in serialized output");
        let zeta_pos = json
            .find("\"zeta.rs\"")
            .expect("zeta.rs should be in serialized output");
        assert!(alpha_pos < middle_pos);
        assert!(middle_pos < zeta_pos);
    }
}
