use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use arboard::Clipboard;
use clap::ValueEnum;
use strum::EnumString;

use crate::discovery::discover;
use crate::files::Files;
use crate::tokenizer::tokenize;
use crate::tree::FiletreeNode;

#[derive(Default, Debug, Clone, Copy, EnumString, ValueEnum, Eq, Hash, PartialEq)]
pub enum CountTokenOptions {
    #[default]
    #[strum(serialize = "none")]
    None,
    #[strum(serialize = "final")]
    Final,
    #[strum(serialize = "all")]
    All,
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
            .map(|r| r.value().meta.token_count.unwrap_or_default())
            .sum::<usize>();
        let total_tokens = total_tokens.to_string();
        println!("Total tokens: {}", total_tokens);
    }
    Ok(())
}

pub async fn output(
    first_path: PathBuf,
    rest_paths: Vec<PathBuf>,
    exclude: Vec<glob::Pattern>,
    stdout: bool,
    no_summary: bool,
    count_tokens: CountTokenOptions,
) -> Result<()> {
    let discovered = discover(first_path.clone(), rest_paths.to_vec(), exclude)?;
    let files =
        Files::read_from(discovered, matches!(count_tokens, CountTokenOptions::All)).await?;

    let tree = FiletreeNode::try_from(&files)?;

    let mut prompt = vec![];
    let excluded = files.get_excluded();

    write_filetree(&mut prompt, &tree)?;
    write_files_content(&mut prompt, files)?;

    let output = String::from_utf8_lossy(&prompt);
    let final_token_count = match count_tokens {
        CountTokenOptions::Final | CountTokenOptions::All => Some(tokenize(&output).len()),
        CountTokenOptions::None => None,
    };

    if stdout {
        print!("{}", output);
    } else {
        let mut clipboard = Clipboard::new()?;
        clipboard.set_text(output)?;
    }
    if no_summary {
        return Ok(());
    }

    write_filetree(std::io::stdout(), &tree)?;
    if let Some(token_count) = final_token_count {
        println!("{} total tokens copied", token_count);
    }
    println!("Excluded: {:?}", excluded);

    Ok(())
}

fn write_filetree(mut writer: impl Write, tree: &FiletreeNode) -> Result<()> {
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
        if info.meta.excluded {
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

fn write_top(mut writer: impl Write, files: &Files, top: u32) -> Result<()> {
    let mut sorted = files.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| b.value().meta.token_count.cmp(&a.value().meta.token_count));
    let mut top_total_tokens = 0;
    let mut top_file_count = 0; // track this in case there are less files in total than top
    let mut all_total_tokens = 0;
    let all_file_count = sorted.len();

    let mut iter = sorted.into_iter();

    for entry in iter.by_ref().take(top as usize) {
        let path = entry.key();
        let token_count = entry
            .value()
            .meta
            .token_count
            .expect("should always be counting tokens when counting top");
        writeln!(writer, "{}: {} tokens", path.display(), token_count)?;
        top_total_tokens += token_count;
        all_total_tokens += token_count;
        top_file_count += 1;
    }
    for entry in iter {
        let token_count = entry
            .value()
            .meta
            .token_count
            .expect("should always be counting tokens when counting top");
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
