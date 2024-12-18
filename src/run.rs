use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use arboard::Clipboard;
use glob::Pattern;
use ignore::WalkBuilder;
use num_format::{Buffer, CustomFormat, Grouping};
use tiktoken_rs::o200k_base_singleton;

use crate::files::{strip_dot_prefix, Files};
use crate::tree::FiletreeNode;

pub fn walk_files(
    path: PathBuf,
    extra_paths: Vec<PathBuf>,
    exclude: Vec<Pattern>,
) -> Result<Files> {
    let files = Files::default();
    let exclude = Arc::new(exclude);

    let mut walker = WalkBuilder::new(path.clone());
    walker.add_custom_ignore_filename(".promptignore");
    for path in extra_paths {
        walker.add(path);
    }
    let walker = walker.build_parallel();

    walker.run(|| {
        let exclude = Arc::clone(&exclude);
        files.mkf(move |path| {
            let path = strip_dot_prefix(path);
            exclude.iter().any(|pattern| pattern.matches_path(path))
        })
    });
    Ok(files)
}

pub fn count(files: Files, top: Option<u32>) -> Result<()> {
    if let Some(count) = top {
        write_top(std::io::stdout(), &files, count)?;
    } else {
        let total_tokens = files
            .iter()
            .map(|r| r.value().meta().token_count())
            .sum::<usize>();
        let total_tokens = total_tokens.to_string();
        println!("Total tokens: {}", total_tokens);
    }
    Ok(())
}

pub fn output(files: Files, stdout: bool) -> Result<()> {
    let tree = FiletreeNode::try_from(&files)?;

    let mut prompt = vec![];

    write_filetree(&mut prompt, &tree)?;
    write_files_content(&mut prompt, files)?;

    let output = String::from_utf8_lossy(&prompt);
    let total_tokens = total_tokens(&output);

    if stdout {
        print!("{}", output);
        return Ok(());
    }

    let mut clipboard = Clipboard::new()?;
    clipboard.set_text(output)?;
    write_filetree(std::io::stdout(), &tree)?;
    println!("{} total tokens copied", total_tokens);

    Ok(())
}

fn total_tokens(text: &str) -> String {
    let tokens = {
        let bpe = o200k_base_singleton();
        let bpe = bpe.lock();
        bpe.encode_with_special_tokens(text)
    };

    let tokens_format = CustomFormat::builder()
        .grouping(Grouping::Standard) // 1000s separation
        .separator("_")
        .build()
        .expect("should be able to build tokens format");
    let mut tokens_formatted = Buffer::default();
    tokens_formatted.write_formatted(&tokens.len(), &tokens_format);

    tokens_formatted.to_string()
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
        if info.meta().excluded() {
            continue;
        }
        writeln!(writer, "{}:", path.display())?;
        writeln!(writer)?;
        writeln!(writer, "{}", info.utf8())?;
        writeln!(writer, "---")?;
    }

    Ok(())
}

fn write_top(mut writer: impl Write, files: &Files, top: u32) -> Result<()> {
    let mut sorted = files.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        b.value()
            .meta()
            .token_count()
            .cmp(&a.value().meta().token_count())
    });
    let mut top_total_tokens = 0;
    let mut top_file_count = 0; // track this in case there are less files in total than top
    let mut all_total_tokens = 0;
    let all_file_count = sorted.len();

    let mut iter = sorted.into_iter();

    for entry in iter.by_ref().take(top as usize) {
        let path = entry.key();
        let token_count = entry.value().meta().token_count();
        writeln!(writer, "{}: {} tokens", path.display(), token_count)?;
        top_total_tokens += token_count;
        all_total_tokens += token_count;
        top_file_count += 1;
    }
    for entry in iter {
        let token_count = entry.value().meta().token_count();
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
