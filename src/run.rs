use std::io::Write;

use anyhow::Result;
use arboard::Clipboard;

use crate::files::Files;
use crate::tokenizer::tokenize;
use crate::tree::FiletreeNode;

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
    let excluded = files.excluded();

    write_filetree(&mut prompt, &tree)?;
    write_files_content(&mut prompt, files)?;

    let output = String::from_utf8_lossy(&prompt);
    let total_tokens = tokenize(&output);

    if stdout {
        print!("{}", output);
        return Ok(());
    }

    let mut clipboard = Clipboard::new()?;
    clipboard.set_text(output)?;
    write_filetree(std::io::stdout(), &tree)?;
    println!("{} total tokens copied", total_tokens.len());
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
