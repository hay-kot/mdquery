use crate::types::{Row, Value};
use anyhow::Result;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Read a markdown file and extract a Row from it.
///
/// If `needs_content` is false, uses a fast path that only reads frontmatter lines.
/// If true, reads the entire file.
pub fn read_file(path: &Path, base_dir: &Path, needs_content: bool) -> Result<Option<Row>> {
    let mut row = if needs_content {
        read_full(path)?
    } else {
        read_frontmatter_only(path)?
    };

    // Add virtual columns
    let relative = path
        .strip_prefix(base_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();
    row.insert("path".into(), Value::String(relative));

    let filename = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    row.insert("filename".into(), Value::String(filename));

    // File metadata — cheap since OS usually caches this from the directory walk
    if let Ok(metadata) = std::fs::metadata(path) {
        row.insert("size".into(), Value::Int(metadata.len() as i64));
        if let Ok(modified) = metadata.modified() {
            let dt: chrono::DateTime<chrono::Local> = modified.into();
            row.insert("modified".into(), Value::Date(dt.date_naive()));
        }
    }

    Ok(Some(row))
}

/// Fast path: read only frontmatter by scanning line-by-line until the closing `---`.
/// Avoids reading the full file and avoids gray_matter overhead.
fn read_frontmatter_only(path: &Path) -> Result<Row> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut row = Row::new();
    let mut in_frontmatter = false;
    let mut yaml_lines = String::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if !in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = true;
                continue;
            }
            // No frontmatter delimiter at start — file has no frontmatter
            return Ok(row);
        }

        // We're inside frontmatter
        if trimmed == "---" {
            // End of frontmatter — parse what we have and stop reading
            break;
        }
        yaml_lines.push_str(&line);
        yaml_lines.push('\n');
    }

    if !yaml_lines.is_empty()
        && let Ok(serde_yml::Value::Mapping(map)) =
            serde_yml::from_str::<serde_yml::Value>(&yaml_lines)
    {
        for (key, val) in map {
            if let serde_yml::Value::String(k) = key {
                row.insert(k, Value::from_yaml(&val));
            }
        }
    }

    Ok(row)
}

/// Full file read path: uses gray_matter for complete frontmatter + content extraction.
fn read_full(path: &Path) -> Result<Row> {
    let raw = std::fs::read_to_string(path)?;
    let mut row = Row::new();

    let matter = gray_matter::Matter::<gray_matter::engine::YAML>::new();
    let result = matter.parse(&raw);

    if let Some(gray_matter::Pod::Hash(map)) = &result.data {
        for (key, pod) in map {
            let value = pod_to_value(pod);
            row.insert(key.clone(), value);
        }
    }

    row.insert("content".into(), Value::String(result.content));
    Ok(row)
}

fn pod_to_value(pod: &gray_matter::Pod) -> Value {
    match pod {
        gray_matter::Pod::Null => Value::Null,
        gray_matter::Pod::Boolean(b) => Value::Bool(*b),
        gray_matter::Pod::Integer(i) => Value::Int(*i),
        gray_matter::Pod::Float(f) => Value::Float(*f),
        gray_matter::Pod::String(s) => {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                return Value::Date(d);
            }
            if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
                return Value::DateTime(dt);
            }
            Value::String(s.clone())
        }
        gray_matter::Pod::Array(arr) => Value::Array(arr.iter().map(pod_to_value).collect()),
        gray_matter::Pod::Hash(map) => {
            let pairs: Vec<String> = map.iter().map(|(k, v)| format!("{k}: {v:?}")).collect();
            Value::String(format!("{{{}}}", pairs.join(", ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn reads_frontmatter_only() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.md");
        fs::write(
            &path,
            "---\ntitle: Hello World\nstatus: draft\ntags:\n  - rust\n  - cli\n---\nSome body content here.\n",
        )
        .unwrap();

        let row = read_file(&path, dir.path(), false).unwrap().unwrap();

        assert_eq!(row.get("title"), Some(&Value::String("Hello World".into())));
        assert_eq!(row.get("status"), Some(&Value::String("draft".into())));
        assert!(matches!(row.get("tags"), Some(Value::Array(_))));
        assert!(row.get("content").is_none());
        assert_eq!(row.get("path"), Some(&Value::String("test.md".into())));
        assert_eq!(row.get("filename"), Some(&Value::String("test".into())));
    }

    #[test]
    fn reads_with_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, "---\ntitle: Test\n---\nBody content here.\n").unwrap();

        let row = read_file(&path, dir.path(), true).unwrap().unwrap();

        assert!(row.get("content").is_some());
        if let Some(Value::String(content)) = row.get("content") {
            assert!(content.contains("Body content here"));
        }
    }

    #[test]
    fn reads_file_without_frontmatter() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("plain.md");
        fs::write(&path, "Just some markdown without frontmatter.\n").unwrap();

        let row = read_file(&path, dir.path(), false).unwrap().unwrap();

        assert_eq!(row.get("path"), Some(&Value::String("plain.md".into())));
        assert_eq!(row.get("filename"), Some(&Value::String("plain".into())));
    }

    #[test]
    fn parses_date_in_frontmatter() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("dated.md");
        fs::write(&path, "---\ndate: 2025-03-15\n---\n").unwrap();

        let row = read_file(&path, dir.path(), false).unwrap().unwrap();
        assert!(matches!(row.get("date"), Some(Value::Date(_))));
    }

    #[test]
    fn fast_path_matches_full_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("compare.md");
        fs::write(
            &path,
            "---\ntitle: Compare\ncount: 42\ntags:\n  - a\n  - b\n---\nBody.\n",
        )
        .unwrap();

        let fast = read_file(&path, dir.path(), false).unwrap().unwrap();
        let full = read_file(&path, dir.path(), true).unwrap().unwrap();

        // Frontmatter fields should match
        assert_eq!(fast.get("title"), full.get("title"));
        assert_eq!(fast.get("count"), full.get("count"));
        assert_eq!(fast.get("tags"), full.get("tags"));
    }
}
