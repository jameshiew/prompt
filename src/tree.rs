use std::collections::BTreeMap;
use std::fmt;

use anyhow::Result;
use crossterm::style::Stylize;
use ptree::TreeItem;

use crate::files::{strip_dot_prefix, FileMeta, Files};

#[derive(Debug, Clone)]
pub(crate) struct FiletreeNode {
    name: String,
    meta: Option<FileMeta>,
    children: BTreeMap<String, FiletreeNode>,
}

impl fmt::Display for FiletreeNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut tree_buf = vec![];
        ptree::write_tree_with(self, &mut tree_buf, &ptree::PrintConfig::default()).map_err(
            |err| {
                tracing::error!(?err, "Couldn't write tree");
                fmt::Error
            },
        )?;
        let tree_str = String::from_utf8_lossy(&tree_buf);
        write!(f, "{}", tree_str)?;
        Ok(())
    }
}

impl FiletreeNode {
    pub(crate) fn new(name: &str, meta: Option<FileMeta>) -> Self {
        Self {
            name: name.to_string(),
            children: BTreeMap::new(),
            meta,
        }
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
                    format!("{} ({} tokens)", style.paint(&self.name), meta.token_count)
                } else if meta.binary_detected {
                    let text = format!(
                        "{} (auto-excluded, binary detected)",
                        style.paint(&self.name)
                    );
                    let text = text.yellow();
                    text.to_string()
                } else {
                    let text = format!("{} (excluded)", style.paint(&self.name));
                    let text = text.red();
                    text.to_string()
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
