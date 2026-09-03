use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use dashmap::DashMap;
use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::Ref;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};

use crate::discovery::DiscoveredFile;
use crate::tokenizer::tokenize;

const BINARY_DETECTION_BYTES: usize = 8 * 1024;
const MAX_CONCURRENT_FILE_READS: usize = 32;
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
    fn new(discovered: DiscoveredFile, count_tokens: bool) -> anyhow::Result<Self> {
        let DiscoveredFile {
            path,
            excluded,
            access,
        } = discovered;
        if excluded {
            return Ok(Self {
                meta: FileMeta {
                    path,
                    read_status: ReadStatus::ExcludedExplicitly,
                },
                utf8: None,
            });
        }

        let mut file = access
            .context("included file should have a filesystem capability")?
            .open()
            .with_context(|| format!("failed to open {}", path.display()))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let sample_len = buffer.len().min(BINARY_DETECTION_BYTES);
        if is_probably_binary(&buffer[..sample_len]) {
            return Ok(Self {
                meta: FileMeta {
                    path,
                    read_status: ReadStatus::ExcludedBinaryDetected,
                },
                utf8: None,
            });
        };
        let text = String::from_utf8(buffer)
            .with_context(|| format!("file {path:?} contains invalid UTF-8"))?;
        let content = annotate_line_numbers(&text);
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
        let mut tasks = tokio::task::JoinSet::new();
        let concurrency = std::thread::available_parallelism()
            .map_or(1, |count| count.get())
            .min(MAX_CONCURRENT_FILE_READS);
        for disc in discovered {
            tasks.spawn_blocking(move || {
                let path = disc.path.clone();
                let info = FileInfo::new(disc, count_tokens)?;
                anyhow::Ok((path, info))
            });
            if tasks.len() >= concurrency
                && let Some(result) = tasks.join_next().await
            {
                let (path, info) = result??;
                files.insert(path, info);
            }
        }
        while let Some(result) = tasks.join_next().await {
            let (path, info) = result??;
            files.insert(path, info);
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

    pub fn excluded_count(&self) -> usize {
        self.inner
            .iter()
            .filter(|entry| entry.value().meta.is_excluded())
            .count()
    }
}

fn annotate_line_numbers(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let line_count = text.split('\n').count();
    let width = line_count.ilog10() as usize + 1;

    let mut numbered = String::new();
    for (i, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line_num = i + 1;
        numbered.push_str(&format!("{line_num:>width$} {line}\n"));
    }

    numbered
}

pub fn strip_dot_prefix(path: &Path) -> &Path {
    path.strip_prefix(".").unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use std::fs as std_fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::discovery::discover;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("prompt-files-test-{unique}"));
            std_fs::create_dir_all(&path).expect("should create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std_fs::remove_dir_all(&self.path);
        }
    }

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

    #[test]
    fn annotate_line_numbers_distinguishes_missing_final_newline() {
        let without_newline = annotate_line_numbers("same content");
        let with_newline = annotate_line_numbers("same content\n");

        assert_ne!(without_newline, with_newline);
        assert_eq!(
            without_newline.lines().collect::<Vec<_>>(),
            vec!["1 same content"]
        );
        let with_lines = with_newline.lines().collect::<Vec<_>>();
        assert_eq!(with_lines.len(), 2);
        assert!(with_lines[0].ends_with("same content"));
        assert_eq!(with_lines[1], "2 ");
    }

    #[test]
    fn annotate_line_numbers_handles_empty_input() {
        assert_eq!(annotate_line_numbers(""), "");
    }

    #[test]
    fn annotate_line_numbers_preserves_multiple_trailing_newlines() {
        let output = annotate_line_numbers("same content\n\n");
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1].trim_start(), "2 ");
        assert_eq!(lines[2].trim_start(), "3 ");
    }

    #[tokio::test]
    async fn valid_utf8_is_read_normally() -> Result<()> {
        let temp = TempDir::new();
        let path = temp.path.join("valid.txt");
        std_fs::write(&path, "café\n")?;

        let discovered = discover(path.clone(), vec![], vec![], false)?;
        let files = Files::read_from(discovered, false).await?;
        let info = files.get(&path).expect("valid.txt should be read");

        assert!(matches!(info.meta.read_status, ReadStatus::Read));
        assert_eq!(info.utf8.as_deref(), Some("1 café\n2 \n"));

        Ok(())
    }

    #[tokio::test]
    async fn invalid_utf8_reports_the_file_path() -> Result<()> {
        let temp = TempDir::new();
        let path = temp.path.join("invalid.txt");
        std_fs::write(&path, b"before\xffafter\n")?;

        let discovered = discover(path.clone(), vec![], vec![], false)?;
        let error = Files::read_from(discovered, false)
            .await
            .err()
            .expect("invalid UTF-8 should fail");
        let message = format!("{error:#}");

        assert!(message.contains("contains invalid UTF-8"));
        assert!(message.contains(path.to_string_lossy().as_ref()));

        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_replacement_after_discovery_is_not_followed() -> Result<()> {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new();
        let selected = temp.path.join("selected.txt");
        let moved = temp.path.join("moved.txt");
        let other = temp.path.join("other.txt");
        std_fs::write(&selected, "selected\n")?;
        std_fs::write(&other, "other\n")?;

        let discovered = discover(selected.clone(), vec![], vec![], false)?;
        std_fs::rename(&selected, moved)?;
        symlink(&other, &selected)?;

        let error = Files::read_from(discovered, false)
            .await
            .err()
            .expect("replacement symlink should not be followed");
        let message = format!("{error:#}");
        assert!(
            message.contains("failed to open"),
            "unexpected error: {message}"
        );
        assert!(message.contains(selected.to_string_lossy().as_ref()));

        Ok(())
    }
}
