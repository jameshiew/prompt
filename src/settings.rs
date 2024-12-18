use std::path::PathBuf;

pub struct Settings {
    pub path: PathBuf,
    pub extra_paths: Vec<PathBuf>,
    pub stdout: bool,
    pub top: Option<u32>,
    pub exclude: Vec<glob::Pattern>,
}
