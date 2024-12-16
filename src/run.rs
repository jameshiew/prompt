use std::io::{stderr, stdout, Write};

use anyhow::Result;
use arboard::Clipboard;
use ignore::WalkBuilder;
use num_format::{Buffer, CustomFormat, Grouping};
use tiktoken_rs::o200k_base_singleton;

use crate::files::Files;
use crate::settings::Settings;
use crate::tree::FiletreeNode;

pub fn start(
    Settings {
        path,
        copy,
        top,
        exclude,
    }: Settings,
) -> Result<()> {
    let files = Files::default();
    WalkBuilder::new(&path)
        .add_custom_ignore_filename(".promptignore")
        .build_parallel()
        .run(|| {
            files.mkf({
                let exclude = exclude.clone();
                move |path| {
                    for pattern in &exclude {
                        let path = path
                            .strip_prefix(".")
                            .expect("should be able to strip prefix");
                        if pattern.matches_path(path) {
                            return true;
                        }
                    }
                    false
                }
            })
        });

    if let Some(count) = top {
        write_top(stdout(), &files, count)?;
        return Ok(());
    }

    let tree = FiletreeNode::try_from(&files)?;

    let mut prompt = vec![];
    let mut stderr = stderr();

    write_files_content(&mut prompt, files)?;
    write_filetree(&mut prompt, &tree)?;
    write_filetree(&mut stderr, &tree)?;

    let output = String::from_utf8_lossy(&prompt);
    let total_tokens = total_tokens(&output);

    if copy {
        let mut clipboard = Clipboard::new()?;
        clipboard.set_text(output)?;
    } else {
        print!("{}", output);
    }

    writeln!(stderr, "{} total tokens", total_tokens)?;

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
        writeln!(writer, "{}:", path.display())?;
        writeln!(writer)?;
        writeln!(writer, "{}", info.utf8())?;
        writeln!(writer, "---")?;
    }

    Ok(())
}

fn write_top(mut writer: impl Write, files: &Files, top: u32) -> Result<()> {
    let mut total = 0;
    let mut sorted = files.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        b.value()
            .meta()
            .token_count()
            .cmp(&a.value().meta().token_count())
    });
    for entry in sorted.iter().take(top as usize) {
        let path = entry.key();
        let token_count = entry.value().meta().token_count();
        writeln!(writer, "{}: {} tokens", path.display(), token_count)?;
        total += token_count;
    }
    writeln!(writer)?;
    writeln!(writer, "{} files", sorted.len())?;
    writeln!(writer, "{} total tokens", total)?;

    Ok(())
}
