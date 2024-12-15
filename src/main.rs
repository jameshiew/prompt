use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::vec;

use anyhow::Result;
use arboard::Clipboard;
use clap::{command, CommandFactory, Parser};
use clap_complete::{generate, Shell};
use dashmap::mapref::multiple::RefMulti;
use dashmap::DashMap;
use ignore::{WalkBuilder, WalkState};
use num_format::{Buffer, CustomFormat, Grouping};
use ptree::TreeItem;
use tiktoken_rs::o200k_base_singleton;
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
    #[arg(long, value_name = "COUNT", help = "List top files by token count")]
    top: Option<u32>,
}

/// Information collected about a read file.
#[derive(Debug)]
struct FileInfo {
    utf8: String,
    token_count: usize,
}

impl FileInfo {
    fn new(bytes: Vec<u8>) -> Result<Self> {
        // TODO: binary detection
        let utf8 = String::from_utf8_lossy(&bytes).to_string();
        let tokens = {
            let bpe = o200k_base_singleton();
            let bpe = bpe.lock();
            bpe.encode_with_special_tokens(&utf8)
        };

        Ok(Self {
            token_count: tokens.len(),
            utf8,
        })
    }

    fn utf8(&self) -> &str {
        &self.utf8
    }
}

#[derive(Debug, Clone, Copy)]
struct FileMeta {
    token_count: usize,
}

impl From<&FileInfo> for FileMeta {
    fn from(info: &FileInfo) -> Self {
        Self {
            token_count: info.token_count,
        }
    }
}

#[derive(Default)]
struct Files {
    inner: DashMap<PathBuf, FileInfo>,
}

impl Files {
    fn insert(&self, path: PathBuf, info: FileInfo) {
        self.inner.insert(path, info);
    }

    fn remove(&self, path: &Path) -> Option<FileInfo> {
        self.inner.remove(path).map(|(_, info)| info)
    }

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn iter(&self) -> impl Iterator<Item = RefMulti<PathBuf, FileInfo>> {
        self.inner.iter()
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("should be able to initialize the logger");

    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, BINARY_NAME, &mut std::io::stdout());
        return Ok(());
    }

    let path = cli.path.unwrap_or_else(|| PathBuf::from("."));
    let all_files = Files::default();

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
                            FileInfo::new(
                                read_file_sync_with_line_numbers(dir_entry.path())
                                    .expect("should be able to read file"),
                            )
                            .expect("should be able to create file info"),
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
    write_output(&mut output, all_files, cli.top)?;

    let output = String::from_utf8_lossy(&output);
    let tokens = {
        let bpe = o200k_base_singleton();
        let bpe = bpe.lock();
        bpe.encode_with_special_tokens(&output)
    };
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
    meta: Option<FileMeta>,
    children: BTreeMap<String, FileNode>,
}

impl FileNode {
    fn new(name: &str, meta: Option<FileMeta>) -> Self {
        Self {
            name: name.to_string(),
            children: BTreeMap::new(),
            meta,
        }
    }

    fn insert_path(&mut self, components: &[&str], meta: Option<FileMeta>) {
        if components.is_empty() {
            return;
        }

        let name = components[0];
        let is_last = components.len() == 1;

        let entry = self.children.entry(name.to_string());
        let entry = if is_last {
            // file node
            entry.or_insert_with(|| FileNode::new(name, meta))
        } else {
            // directory node
            entry.or_insert_with(|| FileNode::new(name, None))
        };

        if !is_last {
            // keep passing info down until final file node reached
            entry.insert_path(&components[1..], meta);
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
        match &self.meta {
            Some(meta) => {
                write!(
                    f,
                    "{} ({} tokens)",
                    style.paint(&self.name),
                    meta.token_count
                )
            }
            None => {
                write!(f, "{}", style.paint(&self.name))
            }
        }
    }

    fn children(&self) -> std::borrow::Cow<[Self::Child]> {
        let children = self.children.values().cloned().collect::<Vec<FileNode>>();
        std::borrow::Cow::Owned(children)
    }
}

fn write_output<W: Write>(mut writer: W, all_files: Files, top: Option<u32>) -> Result<()> {
    let mut keys = all_files
        .iter()
        .map(|r| r.key().clone())
        .collect::<Vec<_>>();
    keys.sort();

    // Build a tree of files collected
    let mut infos: HashMap<PathBuf, FileInfo> = HashMap::default();
    let mut root = FileNode::new(".", None);
    for path in keys.iter() {
        let info = all_files
            .remove(path)
            .expect("should be able to get file contents from map");

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

        root.insert_path(&components, Some((&info).into()));
        infos.insert(path.clone(), info);
    }

    let mut tree_buf = Vec::new();
    ptree::write_tree_with(&root, &mut tree_buf, &ptree::PrintConfig::default())?;
    let tree_str = String::from_utf8_lossy(&tree_buf);

    for path in keys.iter() {
        let info = infos.get(path).expect("should be able to get file info");
        writeln!(writer, "{}:", path.display())?;
        writeln!(writer)?;
        writeln!(writer, "{}", info.utf8())?;
        writeln!(writer, "---")?;
    }
    writeln!(writer, "Files:")?;
    writeln!(writer)?;
    writeln!(writer, "{}", tree_str)?;
    writeln!(writer)?;
    writeln!(writer, "{} files", keys.len())?;

    if let Some(top) = top {
        let mut total = 0;
        let mut sorted = infos
            .iter()
            .map(|(path, info)| (path, info.token_count))
            .collect::<Vec<_>>();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        for (path, token_count) in sorted.into_iter().take(top as usize) {
            let path = path.as_path();
            writeln!(writer, "{}: {} tokens", path.display(), token_count)?;
            total += token_count;
        }
        writeln!(writer, "{} total tokens", total)?;
    }

    Ok(())
}
