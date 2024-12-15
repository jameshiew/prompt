use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{command, Parser};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version)]
struct Cli {
    path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("should be able to initialize the logger");

    let cli = Cli::parse();
    let path = cli.path.unwrap_or_else(|| PathBuf::from("."));
    let mut all_files = Vec::new();

    read_files_iteratively(path.as_path(), &mut all_files).await?;

    tracing::info!("Read {} files into memory.", all_files.len());
    print_files(all_files);
    Ok(())
}

async fn read_file(path: &Path) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path).await?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await?;
    Ok(buffer)
}

async fn read_files_iteratively(
    path: &Path,
    all_files: &mut Vec<(PathBuf, Vec<u8>)>,
) -> Result<()> {
    let mut stack = vec![path.to_path_buf()];

    while let Some(current_path) = stack.pop() {
        let metadata = current_path.metadata()?;
        if metadata.is_dir() {
            let mut dir = fs::read_dir(&current_path).await?;
            while let Some(entry) = dir.next_entry().await? {
                let file_type = entry.file_type().await?;
                let file_path = entry.path();

                if file_type.is_dir() {
                    stack.push(file_path);
                } else if file_type.is_file() {
                    let content = read_file(&file_path).await?;
                    all_files.push((file_path, content));
                }
            }
        } else if metadata.is_file() {
            let content = read_file(&path).await?;
            all_files.push((current_path, content));
        }
    }

    Ok(())
}

fn print_files(all_files: Vec<(PathBuf, Vec<u8>)>) {
    for (path, buffer) in all_files {
        println!("{}:", path.display());
        println!("");
        println!("{}", String::from_utf8_lossy(&buffer));
        println!("---");
    }
}
