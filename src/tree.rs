use std::collections::BTreeMap;

use anyhow::Result;
use ptree::print_config::StyleWhen;
use ptree::TreeItem;

use crate::files::{strip_dot_prefix, FileMeta, Files};

#[derive(Debug, Clone)]
pub(crate) struct FiletreeNode {
    name: String,
    meta: Option<FileMeta>,
    children: BTreeMap<String, FiletreeNode>,
}

impl FiletreeNode {
    pub(crate) fn new(name: &str, meta: Option<FileMeta>) -> Self {
        Self {
            name: name.to_string(),
            children: BTreeMap::new(),
            meta,
        }
    }

    fn ptree(&self, cfg: &ptree::PrintConfig) -> Result<String> {
        let mut buf = vec![];
        ptree::write_tree_with(self, &mut buf, cfg)?;
        Ok(String::from_utf8_lossy(&buf).to_string())
    }

    pub(crate) fn tty_output(&self) -> Result<String> {
        self.ptree(&ptree::PrintConfig {
            styled: StyleWhen::Tty,
            ..ptree::PrintConfig::default()
        })
    }

    pub(crate) fn plain_output(&self) -> Result<String> {
        self.ptree(&ptree::PrintConfig {
            styled: StyleWhen::Never,
            ..ptree::PrintConfig::default()
        })
    }

    pub(crate) fn insert_path(&mut self, components: &[&str], meta: Option<FileMeta>) {
        if components.is_empty() {
            return;
        }

        let name = components[0];
        let is_last = components.len() == 1;

        let entry = self.children.entry(name.to_string());
        let entry = if is_last {
            // file node
            let meta = meta.clone();
            entry.or_insert_with(|| FiletreeNode::new(name, meta))
        } else {
            // directory node
            entry.or_insert_with(|| FiletreeNode::new(name, None))
        };

        if !is_last {
            entry.insert_path(&components[1..], meta);
        }
    }
}

impl TreeItem for FiletreeNode {
    type Child = FiletreeNode;

    fn write_self<W: std::io::Write>(
        &self,
        f: &mut W,
        style: &ptree::Style,
    ) -> std::io::Result<()> {
        match &self.meta {
            Some(meta) => {
                let text = if !meta.excluded {
                    match meta.token_count {
                        Some(token_count) => {
                            format!("{} ({} tokens)", &self.name, token_count)
                        }
                        None => self.name.to_owned(),
                    }
                } else if meta.binary_detected {
                    format!("{} (auto-excluded, binary detected)", &self.name)
                } else {
                    format!("{} (excluded)", &self.name)
                };
                write!(f, "{}", style.paint(text))
            }
            None => {
                write!(f, "{}", style.paint(&self.name))
            }
        }
    }

    fn children(&self) -> std::borrow::Cow<[Self::Child]> {
        let children = self
            .children
            .values()
            .cloned()
            .collect::<Vec<FiletreeNode>>();
        std::borrow::Cow::Owned(children)
    }
}

impl TryFrom<&Files> for FiletreeNode {
    type Error = anyhow::Error;

    fn try_from(files: &Files) -> Result<Self> {
        let paths = files.iter().map(|r| r.key().clone());

        // Build a tree of files collected
        let mut root = FiletreeNode::new(".", None);
        for path in paths {
            let meta = files
                .get(&path)
                .expect("should be able to get file contents from map")
                .value()
                .meta
                .clone();

            // Remove leading "./" since the root node is the "."
            let path = strip_dot_prefix(&path);

            let components = path
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect::<Vec<_>>();

            root.insert_path(&components, Some(meta));
        }
        Ok(root)
    }
}
