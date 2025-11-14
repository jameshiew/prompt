use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use arboard::Clipboard;
use clap::ValueEnum;
use serde::Serialize;
use strum::EnumString;

use crate::discovery::discover;
use crate::files::{Files, ReadStatus};
use crate::tokenizer::tokenize;
use crate::tree::FiletreeNode;

#[derive(Default, Debug, Clone, Copy, EnumString, ValueEnum, Eq, Hash, PartialEq)]
pub enum TokenCountOptions {
    #[strum(serialize = "none")]
    None,
    #[default]
    #[strum(serialize = "final")]
    Final,
    #[strum(serialize = "all")]
    All,
}

#[derive(
    Default, Debug, strum::Display, Clone, Copy, EnumString, ValueEnum, Eq, Hash, PartialEq,
)]
pub enum Format {
    #[default]
    #[strum(serialize = "plaintext")]
    Plaintext,
    #[strum(serialize = "json")]
    Json,
    #[strum(serialize = "yaml")]
    Yaml,
}

pub async fn count(
    first_path: PathBuf,
    rest_paths: Vec<PathBuf>,
    exclude: Vec<glob::Pattern>,
    include_gitignored: bool,
    top: Option<u32>,
) -> Result<()> {
    let discovered = discover(
        first_path.clone(),
        rest_paths.to_vec(),
        exclude,
        include_gitignored,
    )?;
    let files = Files::read_from(discovered, true).await?;

    if let Some(count) = top {
        write_top(std::io::stdout(), &files, count)?;
    } else {
        let total_tokens = files
            .iter()
            .map(|r| {
                let info = r.value();
                match info.meta.read_status {
                    ReadStatus::ExcludedExplicitly | ReadStatus::ExcludedBinaryDetected => 0,
                    ReadStatus::Read => unreachable!(
                        "non-excluded files should have token count: {}",
                        info.meta.path.display()
                    ),
                    ReadStatus::TokenCounted(token_count) => token_count,
                }
            })
            .sum::<usize>();
        let total_tokens = total_tokens.to_string();
        println!("Total tokens: {total_tokens}");
    }
    Ok(())
}

#[derive(Serialize)]
struct Output {
    tree: String,
    files: Files,
}

pub async fn generate(
    first_path: PathBuf,
    rest_paths: Vec<PathBuf>,
    exclude: Vec<glob::Pattern>,
    no_gitignore: bool,
    stdout: bool,
    token_count: TokenCountOptions,
    format: Format,
) -> Result<()> {
    let discovered = discover(
        first_path.clone(),
        rest_paths.to_vec(),
        exclude,
        no_gitignore,
    )?;
    let files = Files::read_from(discovered, matches!(token_count, TokenCountOptions::All)).await?;

    let tree = FiletreeNode::try_from(&files)?;

    let excluded = files.get_excluded();

    let output = match format {
        Format::Plaintext => {
            let mut prompt = vec![];
            write_filetree(&mut prompt, tree.tty_output()?)?;
            write_files_content(&mut prompt, files)?;
            String::from_utf8_lossy(&prompt).into_owned()
        }
        Format::Json => serde_json::to_string(&Output {
            tree: tree.tty_output()?,
            files,
        })?,
        Format::Yaml => serde_norway::to_string(&Output {
            tree: tree.tty_output()?,
            files,
        })?,
    };

    let final_token_count = match token_count {
        TokenCountOptions::Final | TokenCountOptions::All => Some(tokenize(&output).len()),
        TokenCountOptions::None => None,
    };

    if stdout {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(output.as_bytes())?;
        handle.flush()?;
        return Ok(()); // no summary if printing prompt to stdout
    }

    let mut clipboard = Clipboard::new()?;
    clipboard.set_text(output)?;

    write_filetree(std::io::stdout(), tree.tty_output()?)?;
    if let Some(token_count) = final_token_count {
        println!("{token_count} total tokens copied ({format})");
    }
    if !excluded.is_empty() {
        println!("Excluded {} files: {:?}", excluded.len(), excluded);
    }

    Ok(())
}

fn write_filetree(mut writer: impl Write, tree: String) -> Result<()> {
    writeln!(writer, "Files:")?;
    writeln!(writer)?;
    writeln!(writer, "{tree}")?;
    Ok(())
}

fn write_files_content(mut writer: impl Write, files: Files) -> Result<()> {
    let mut paths = files.iter().map(|r| r.key().clone()).collect::<Vec<_>>();
    paths.sort();
    for path in paths.iter() {
        let info = files.remove(path).expect("should be able to get file info");
        if info.meta.is_excluded() {
            continue;
        }
        writeln!(writer, "{}:", path.display())?;
        writeln!(writer)?;
        writeln!(
            writer,
            "{}",
            info.utf8
                .expect("should be able to get utf8 if this file wasn't excluded")
        )?;
        writeln!(writer, "---")?;
    }

    Ok(())
}

#[allow(clippy::significant_drop_tightening)]
fn write_top(mut writer: impl Write, files: &Files, top: u32) -> Result<()> {
    let mut entries = files
        .iter()
        .filter(|entry| !entry.value().meta.is_excluded())
        .collect::<Vec<_>>();

    let skipped_files = files.len().saturating_sub(entries.len());
    let all_file_count = entries.len();

    entries.sort_by(|a, b| {
        b.value()
            .meta
            .token_count_or_zero()
            .cmp(&a.value().meta.token_count_or_zero())
    });

    let mut top_total_tokens = 0;
    let mut top_file_count = 0;
    let mut all_total_tokens = 0;

    for entry in entries.iter().take(top as usize) {
        let path = entry.key();
        let token_count = entry.value().meta.token_count_or_zero();
        writeln!(writer, "{}: {} tokens", path.display(), token_count)?;
        top_total_tokens += token_count;
        all_total_tokens += token_count;
        top_file_count += 1;
    }

    for entry in entries.iter().skip(top as usize) {
        let token_count = entry.value().meta.token_count_or_zero();
        all_total_tokens += token_count;
    }

    writeln!(writer)?;
    writeln!(
        writer,
        "Top {top_file_count} files = {top_total_tokens} tokens",
    )?;
    writeln!(
        writer,
        "All {all_file_count} files = {all_total_tokens} tokens"
    )?;
    if skipped_files > 0 {
        writeln!(
            writer,
            "{skipped_files} files skipped (excluded or binary detected)"
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::Result;

    use super::*;
    use crate::discovery::DiscoveredFile;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("prompt-run-test-{unique}"));
            fs::create_dir_all(&path).expect("should create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[tokio::test]
    async fn write_top_omits_excluded_files() -> Result<()> {
        let temp = TempDir::new();
        let included_path = temp.path.join("included.txt");
        fs::write(&included_path, b"hello")?;

        let discovered = vec![
            DiscoveredFile {
                path: included_path.clone(),
                excluded: false,
            },
            DiscoveredFile {
                path: temp.path.join("target/excluded.bin"),
                excluded: true,
            },
        ];

        let files = Files::read_from(discovered, true).await?;

        let mut buffer = Vec::new();
        write_top(&mut buffer, &files, 5)?;
        let output = String::from_utf8(buffer).expect("valid utf8 output");

        assert!(output.contains("included.txt"));
        assert!(!output.contains("excluded.bin"));
        assert!(output.contains("Top 1 files ="));
        assert!(output.contains("All 1 files ="));
        assert!(output.contains("1 files skipped"));

        Ok(())
    }
}
