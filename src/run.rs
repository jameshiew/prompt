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
    top: Option<u32>,
) -> Result<()> {
    let discovered = discover(first_path.clone(), rest_paths.to_vec(), exclude)?;
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
        println!("Total tokens: {}", total_tokens);
    }
    Ok(())
}

#[derive(Serialize)]
struct Output {
    tree: String,
    files: Files,
}

pub async fn output(
    first_path: PathBuf,
    rest_paths: Vec<PathBuf>,
    exclude: Vec<glob::Pattern>,
    stdout: bool,
    token_count: TokenCountOptions,
    format: Format,
) -> Result<()> {
    let discovered = discover(first_path.clone(), rest_paths.to_vec(), exclude)?;
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
        Format::Yaml => serde_yml::to_string(&Output {
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

    // Try to copy to clipboard, but gracefully handle failure in headless environments
    let clipboard_success = match Clipboard::new() {
        Ok(mut clipboard) => match clipboard.set_text(output) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("Warning: Failed to copy to clipboard: {}", e);
                eprintln!(
                    "Hint: Use --stdout flag to output directly instead of copying to clipboard"
                );
                false
            }
        },
        Err(e) => {
            eprintln!("Warning: Failed to access clipboard: {}", e);
            eprintln!("Hint: Use --stdout flag to output directly instead of copying to clipboard");
            false
        }
    };

    write_filetree(std::io::stdout(), tree.tty_output()?)?;
    if let Some(token_count) = final_token_count {
        if clipboard_success {
            println!("{} total tokens copied ({})", token_count, format);
        } else {
            println!("{} total tokens prepared ({})", token_count, format);
        }
    }
    if !excluded.is_empty() {
        println!("Excluded {} files: {:?}", excluded.len(), excluded);
    }

    Ok(())
}

fn write_filetree(mut writer: impl Write, tree: String) -> Result<()> {
    writeln!(writer, "Files:")?;
    writeln!(writer)?;
    writeln!(writer, "{}", tree)?;
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
    let mut top_total_tokens = 0;
    let mut top_file_count = 0; // track this in case there are less files in total than top
    let mut all_total_tokens = 0;
    let all_file_count = files.len();

    let mut iter = {
        let mut sorted = files.iter().collect::<Vec<_>>();
        sorted.sort_by(|a, b| {
            b.value()
                .meta
                .token_count_or_zero()
                .cmp(&a.value().meta.token_count_or_zero())
        });
        sorted.into_iter()
    };

    for entry in iter.by_ref().take(top as usize) {
        let path = entry.key();
        let token_count = entry.value().meta.token_count_or_zero();
        writeln!(writer, "{}: {} tokens", path.display(), token_count)?;
        top_total_tokens += token_count;
        all_total_tokens += token_count;
        top_file_count += 1;
    }
    for entry in iter {
        let token_count = entry.value().meta.token_count_or_zero();
        all_total_tokens += token_count;
    }

    writeln!(writer)?;
    writeln!(
        writer,
        "Top {} files = {} tokens",
        top_file_count, top_total_tokens,
    )?;
    writeln!(
        writer,
        "All {} files = {} tokens",
        all_file_count, all_total_tokens
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn test_output_with_stdout_flag() {
        // Create a temporary directory with a test file
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello, world!").expect("Failed to write test file");

        // Test with stdout flag - should not attempt clipboard access
        let result = output(
            temp_dir.path().to_path_buf(),
            vec![],
            vec![],
            true, // stdout = true
            TokenCountOptions::None,
            Format::Plaintext,
        )
        .await;

        assert!(result.is_ok(), "output() should succeed with stdout flag");
    }
}
