mod filter;
mod sort;

use crate::parser::{Expr, Query, SelectItem};
use crate::scanner;
use crate::types::{Row, Value};
use anyhow::Result;
use rayon::prelude::*;
use std::path::Path;

pub use filter::eval_expr;

/// Execute a parsed query against a directory of markdown files.
pub fn execute(query: &Query, base_dir: &Path) -> Result<Vec<Row>> {
    let files = scanner::find_markdown_files(base_dir)?;

    let mut rows: Vec<Row> = files
        .par_iter()
        .filter_map(|path| {
            let row = scanner::read_file(path, base_dir, query.needs_content).ok()??;

            // Apply WHERE filter
            if let Some(ref filter) = query.filter
                && !filter::eval_expr(filter, &row)
            {
                return None;
            }

            Some(row)
        })
        .collect();

    // Handle aggregate queries (GROUP BY or standalone COUNT)
    if query.is_aggregate {
        return Ok(aggregate(
            &rows,
            &query.columns,
            &query.group_by,
            &query.order_by,
        ));
    }

    // Apply ORDER BY
    if !query.order_by.is_empty() {
        sort::sort_rows(&mut rows, &query.order_by);
    }

    // Apply OFFSET
    if let Some(offset) = query.offset {
        if offset >= rows.len() {
            rows.clear();
        } else {
            rows = rows.into_iter().skip(offset).collect();
        }
    }

    // Apply LIMIT
    if let Some(limit) = query.limit {
        rows.truncate(limit);
    }

    // Project columns
    let mut rows: Vec<Row> = rows
        .into_iter()
        .map(|row| project(&row, &query.columns))
        .collect();

    // Apply DISTINCT
    if query.distinct {
        deduplicate(&mut rows);
    }

    Ok(rows)
}

fn aggregate(
    rows: &[Row],
    columns: &[SelectItem],
    group_by: &[Expr],
    order_by: &[crate::parser::OrderBy],
) -> Vec<Row> {
    use std::collections::BTreeMap;

    if group_by.is_empty() {
        // Simple aggregate: COUNT(*) over all rows
        let mut result = Row::new();
        for col in columns {
            if let SelectItem::Function { name, .. } = col
                && name == "COUNT"
            {
                result.insert("COUNT(*)".into(), Value::Int(rows.len() as i64));
            }
        }
        return vec![result];
    }

    // GROUP BY: bucket rows by group key
    let mut groups: BTreeMap<Vec<String>, Vec<&Row>> = BTreeMap::new();
    for row in rows {
        let key: Vec<String> = group_by
            .iter()
            .map(|expr| match expr {
                Expr::Column(name) => row
                    .get(name)
                    .map(|v| v.display())
                    .unwrap_or_else(|| "NULL".to_string()),
                _ => "NULL".to_string(),
            })
            .collect();
        groups.entry(key).or_default().push(row);
    }

    let mut result_rows: Vec<Row> = groups
        .into_values()
        .map(|group_rows| {
            let mut result = Row::new();
            let sample = group_rows[0];

            for col in columns {
                match col {
                    SelectItem::Column(name) => {
                        let value = sample.get(name).cloned().unwrap_or(Value::Null);
                        result.insert(name.clone(), value);
                    }
                    SelectItem::Function { name, .. } if name == "COUNT" => {
                        result.insert("COUNT(*)".into(), Value::Int(group_rows.len() as i64));
                    }
                    _ => {}
                }
            }
            result
        })
        .collect();

    if !order_by.is_empty() {
        sort::sort_rows(&mut result_rows, order_by);
    }

    result_rows
}

/// Remove duplicate rows based on all field values.
fn deduplicate(rows: &mut Vec<Row>) {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    rows.retain(|row| {
        // Use display values of all fields as the dedup key
        let key: Vec<String> = row
            .fields
            .iter()
            .map(|(k, v)| format!("{k}={}", v.display()))
            .collect();
        let key_str = key.join("|");
        seen.insert(key_str)
    });
}

/// Project a row down to only the selected columns.
fn project(row: &Row, columns: &[SelectItem]) -> Row {
    let mut result = Row::new();

    for col in columns {
        match col {
            SelectItem::Column(name) => {
                let value = row.get(name).cloned().unwrap_or(Value::Null);
                result.insert(name.clone(), value);
            }
            SelectItem::AllColumns => {
                return row.clone();
            }
            SelectItem::Function { name, args } => {
                let value = eval_select_function(name, args, row);
                let display_name = select_function_display_name(name, args);
                result.insert(display_name, value);
            }
        }
    }

    result
}

fn eval_select_function(name: &str, args: &[Expr], row: &Row) -> Value {
    match name {
        "SNIPPET" => {
            if args.len() != 2 {
                return Value::Null;
            }
            let content = filter::eval_to_value_pub(&args[0], row);
            let query = filter::eval_to_value_pub(&args[1], row);
            match (&content, &query) {
                (Value::String(text), Value::String(q)) => {
                    Value::String(extract_snippet(text, q, 80))
                }
                _ => Value::Null,
            }
        }
        "MATCH" => {
            if args.len() != 2 {
                return Value::Bool(false);
            }
            filter::eval_function_pub("MATCH", args, row)
        }
        "LOWER" | "UPPER" | "LEN" => filter::eval_function_pub(name, args, row),
        _ => Value::Null,
    }
}

fn select_function_display_name(name: &str, args: &[Expr]) -> String {
    match name {
        "COUNT" => "COUNT(*)".into(),
        "SNIPPET" => {
            if let Some(Expr::Literal(Value::String(q))) = args.get(1) {
                format!("SNIPPET('{q}')")
            } else {
                "SNIPPET".into()
            }
        }
        _ => name.to_string(),
    }
}

/// Extract a snippet of text around a query match.
fn extract_snippet(text: &str, query: &str, context_chars: usize) -> String {
    let lower_text = text.to_lowercase();
    let lower_query = query.to_lowercase();

    if let Some(pos) = lower_text.find(&lower_query) {
        let start = pos.saturating_sub(context_chars);
        let end = (pos + query.len() + context_chars).min(text.len());

        // Align to word boundaries
        let start = if start > 0 {
            text[start..]
                .find(char::is_whitespace)
                .map(|i| start + i + 1)
                .unwrap_or(start)
        } else {
            0
        };
        let end = if end < text.len() {
            text[..end].rfind(char::is_whitespace).unwrap_or(end)
        } else {
            text.len()
        };

        let mut snippet = String::new();
        if start > 0 {
            snippet.push_str("...");
        }
        snippet.push_str(text[start..end].trim());
        if end < text.len() {
            snippet.push_str("...");
        }
        snippet
    } else {
        // No exact match, return beginning of text
        let end = context_chars.min(text.len());
        let end = text[..end].rfind(char::is_whitespace).unwrap_or(end);
        let mut snippet = text[..end].trim().to_string();
        if end < text.len() {
            snippet.push_str("...");
        }
        snippet
    }
}

/// Get the column names for the result set, preserving SELECT order.
pub fn column_names(query: &Query, sample: Option<&Row>) -> Vec<String> {
    let mut names = Vec::new();
    for col in &query.columns {
        match col {
            SelectItem::Column(name) => names.push(name.clone()),
            SelectItem::AllColumns => {
                if let Some(row) = sample {
                    for key in row.fields.keys() {
                        names.push(key.clone());
                    }
                }
            }
            SelectItem::Function { name, args } => {
                names.push(select_function_display_name(name, args));
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("a.md"),
            "---\ntitle: Alpha\nstatus: published\ndate: 2025-01-01\ntags:\n  - rust\n  - cli\n---\nAlpha content about async runtimes in Rust.\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("b.md"),
            "---\ntitle: Beta\nstatus: draft\ndate: 2025-06-15\ntags:\n  - go\n---\nBeta content about Go channels.\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("c.md"),
            "---\ntitle: Gamma\nstatus: published\ndate: 2025-03-10\n---\nGamma content about error handling patterns.\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn select_all_rows() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT title FROM .").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn select_with_where() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT title FROM . WHERE status = 'draft'").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("title"), Some(&Value::String("Beta".into())));
    }

    #[test]
    fn select_with_order_by() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT title FROM . ORDER BY title ASC").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get("title"), Some(&Value::String("Alpha".into())));
        assert_eq!(rows[1].get("title"), Some(&Value::String("Beta".into())));
        assert_eq!(rows[2].get("title"), Some(&Value::String("Gamma".into())));
    }

    #[test]
    fn select_with_limit() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT title FROM . ORDER BY title ASC LIMIT 2").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn select_with_offset() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT title FROM . ORDER BY title ASC LIMIT 1 OFFSET 1")
            .unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("title"), Some(&Value::String("Beta".into())));
    }

    #[test]
    fn select_star() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT * FROM .").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].get("path").is_some());
        assert!(rows[0].get("content").is_some());
    }

    #[test]
    fn projects_only_selected_columns() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT title FROM . WHERE status = 'published'").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        for row in &rows {
            assert!(row.get("title").is_some());
            assert!(row.get("status").is_none());
        }
    }

    #[test]
    fn count_star() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT COUNT(*) FROM .").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("COUNT(*)"), Some(&Value::Int(3)));
    }

    #[test]
    fn count_with_where() {
        let dir = setup_test_dir();
        let query =
            crate::parser::parse("SELECT COUNT(*) FROM . WHERE status = 'published'").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("COUNT(*)"), Some(&Value::Int(2)));
    }

    #[test]
    fn contains_operator() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT title FROM . WHERE tags CONTAINS 'rust'").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("title"), Some(&Value::String("Alpha".into())));
    }

    #[test]
    fn file_metadata_columns() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT title, size, modified FROM .").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        for row in &rows {
            assert!(matches!(row.get("size"), Some(Value::Int(_))));
            assert!(matches!(row.get("modified"), Some(Value::Date(_))));
        }
    }

    #[test]
    fn snippet_extraction() {
        let text =
            "This is a long document about async runtimes in Rust and how they work with futures.";
        let snippet = extract_snippet(text, "async runtimes", 20);
        assert!(snippet.contains("async runtimes"));
    }

    #[test]
    fn match_in_where() {
        let dir = setup_test_dir();
        let query =
            crate::parser::parse("SELECT title FROM . WHERE MATCH(content, 'async runtimes')")
                .unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("title"), Some(&Value::String("Alpha".into())));
    }

    #[test]
    fn in_list() {
        let dir = setup_test_dir();
        let query = crate::parser::parse(
            "SELECT title FROM . WHERE status IN ('draft', 'review') ORDER BY title",
        )
        .unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("title"), Some(&Value::String("Beta".into())));
    }

    #[test]
    fn not_in_list() {
        let dir = setup_test_dir();
        let query = crate::parser::parse(
            "SELECT title FROM . WHERE status NOT IN ('draft') ORDER BY title",
        )
        .unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn distinct_values() {
        let dir = setup_test_dir();
        let query = crate::parser::parse("SELECT DISTINCT status FROM . ORDER BY status").unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 2);
        let statuses: Vec<_> = rows
            .iter()
            .map(|r| r.get("status").unwrap().display())
            .collect();
        assert_eq!(statuses, vec!["draft", "published"]);
    }

    #[test]
    fn group_by_count() {
        let dir = setup_test_dir();
        let query =
            crate::parser::parse("SELECT status, COUNT(*) FROM . GROUP BY status ORDER BY status")
                .unwrap();
        let rows = execute(&query, dir.path()).unwrap();
        assert_eq!(rows.len(), 2);

        // draft: 1, published: 2
        let draft = rows
            .iter()
            .find(|r| r.get("status") == Some(&Value::String("draft".into())))
            .unwrap();
        assert_eq!(draft.get("COUNT(*)"), Some(&Value::Int(1)));

        let published = rows
            .iter()
            .find(|r| r.get("status") == Some(&Value::String("published".into())))
            .unwrap();
        assert_eq!(published.get("COUNT(*)"), Some(&Value::Int(2)));
    }
}
