use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::vec;

use anyhow::Result;
use arboard::Clipboard;
use clap::{command, CommandFactory, Parser};
use clap_complete::{generate, Shell};
use dashmap::DashMap;
use ignore::{WalkBuilder, WalkState};
use num_format::{Buffer, CustomFormat, Grouping};
use ptree::TreeItem;
use tiktoken_rs::o200k_base;
use tracing_subscriber::EnvFilter;

const BINARY_NAME: &str = "prompt";

#[derive(Parser)]
#[command(version)]
struct Cli {
    #[arg(help = "Path to the file or directory to read into a prompt")]
    path: Option<PathBuf>,
    #[arg(long, value_name = "SHELL", help = "Generate shell completions")]
    completions: Option<Shell>,
    #[arg(long, help = "Copy output straight to the clipboard")]
    copy: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("should be able to initialize the logger");

    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        // let shell = shell.parse::<Shell>().expect("invalid shell name");
        generate(shell, &mut cmd, BINARY_NAME, &mut std::io::stdout());
        return Ok(());
    }

    let path = cli.path.unwrap_or_else(|| PathBuf::from("."));
    let all_files = DashMap::new();

    WalkBuilder::new(&path)
        .add_custom_ignore_filename(".promptignore")
        .build_parallel()
        .run(|| {
            Box::new(|result| {
                match result {
                    Ok(dir_entry) => {
                        if dir_entry.path().is_dir() {
                            return WalkState::Continue;
                        }
                        all_files.insert(
                            dir_entry.path().to_path_buf(),
                            read_file_sync_with_line_numbers(dir_entry.path())
                                .expect("should be able to read file"),
                        );
                    }
                    Err(err) => {
                        panic!("Error reading file: {}", err);
                    }
                }
                WalkState::Continue
            })
        });

    tracing::info!("Read {} files", all_files.len());
    let mut output = vec![];
    write_output(all_files, &mut output)?;

    let output = String::from_utf8_lossy(&output);
    let bpe = o200k_base()?;
    let tokens = bpe.encode_with_special_tokens(&output);
    let tokens_format = CustomFormat::builder()
        .grouping(Grouping::Standard) // 1000s separation
        .separator("_")
        .build()
        .unwrap();
    let mut tokens_formatted = Buffer::default();
    tokens_formatted.write_formatted(&tokens.len(), &tokens_format);

    if cli.copy {
        let mut clipboard = Clipboard::new()?;
        clipboard.set_text(output)?;
    } else {
        print!("{}", output);
    }
    println!("Tokens: {}", tokens_formatted);
    Ok(())
}

fn read_file_sync(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}

fn read_file_sync_with_line_numbers(path: &Path) -> Result<Vec<u8>> {
    let buffer = read_file_sync(path)?;
    let text = String::from_utf8_lossy(&buffer);
    let line_count = text.lines().count();
    if line_count == 0 {
        return Ok(Vec::new());
    }

    let digits = ((line_count as f64).log10().floor() as usize) + 1;
    let width = digits;

    let mut numbered = String::new();
    for (i, line) in text.lines().enumerate() {
        let line_num = i + 1;
        // Right-align the line number within the given width
        numbered.push_str(&format!("{:>width$} {}\n", line_num, line, width = width));
    }

    Ok(numbered.into_bytes())
}

#[derive(Debug, Clone)]
struct FileNode {
    name: String,
    children: BTreeMap<String, FileNode>,
}

impl FileNode {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            children: BTreeMap::new(),
        }
    }

    fn insert_path(&mut self, path: &[&str]) {
        if path.is_empty() {
            return;
        }

        let part = path[0];
        let is_last = path.len() == 1;
        let entry = self
            .children
            .entry(part.to_string())
            .or_insert_with(|| FileNode::new(part));

        if !is_last {
            entry.insert_path(&path[1..]);
        }
    }
}

impl TreeItem for FileNode {
    type Child = FileNode;

    fn write_self<W: std::io::Write>(
        &self,
        f: &mut W,
        style: &ptree::Style,
    ) -> std::io::Result<()> {
        write!(f, "{}", style.paint(&self.name))
    }

    fn children(&self) -> std::borrow::Cow<[Self::Child]> {
        let children = self.children.values().cloned().collect::<Vec<FileNode>>();
        std::borrow::Cow::Owned(children)
    }
}

fn write_output<W: Write>(
    all_files: dashmap::DashMap<PathBuf, Vec<u8>>,
    mut writer: W,
) -> Result<()> {
    let mut keys = all_files
        .iter()
        .map(|r| r.key().clone())
        .collect::<Vec<_>>();
    keys.sort();

    // Build a tree of files collected
    let mut root = FileNode::new(".");
    for path in &keys {
        let mut components = path
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect::<Vec<_>>();

        // Remove leading "./" since the root node is the "."
        if let Some(first) = components.first() {
            if *first == "." {
                components.remove(0);
            }
        }

        root.insert_path(&components);
    }

    let mut tree_buf = Vec::new();
    ptree::write_tree_with(&root, &mut tree_buf, &ptree::PrintConfig::default())?;
    let tree_str = String::from_utf8_lossy(&tree_buf);

    // Now proceed with writing out the file contents as before
    for path in keys.iter() {
        writeln!(writer, "{}:", path.display())?;
        writeln!(writer)?;
        let contents = all_files
            .get(path)
            .expect("should be able to get file contents from map");
        writeln!(writer, "{}", String::from_utf8_lossy(&contents))?;
        writeln!(writer, "---")?;
    }
    writeln!(writer, "Files:")?;
    writeln!(writer)?;
    writeln!(writer, "{}", tree_str)?;
    writeln!(writer)?;
    writeln!(writer, "{} files", keys.len())?;

    Ok(())
}
