use std::path::PathBuf;

pub struct Settings {
    pub path: PathBuf,
    pub copy: bool,
    pub top: Option<u32>,
}