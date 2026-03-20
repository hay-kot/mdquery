mod reader;

pub use reader::read_file;

use anyhow::Result;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

/// Discover all .md files under the given directory, respecting .gitignore.
pub fn find_markdown_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let walker = WalkBuilder::new(dir).hidden(false).build();

    for entry in walker {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            files.push(path.to_path_buf());
        }
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn finds_md_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("note1.md"), "---\ntitle: A\n---\n").unwrap();
        fs::write(dir.path().join("note2.md"), "---\ntitle: B\n---\n").unwrap();
        fs::write(dir.path().join("ignore.txt"), "not markdown").unwrap();

        let files = find_markdown_files(dir.path()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.extension().unwrap() == "md"));
    }

    #[test]
    fn finds_nested_md_files() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(dir.path().join("root.md"), "---\ntitle: R\n---\n").unwrap();
        fs::write(sub.join("nested.md"), "---\ntitle: N\n---\n").unwrap();

        let files = find_markdown_files(dir.path()).unwrap();
        assert_eq!(files.len(), 2);
    }
}
