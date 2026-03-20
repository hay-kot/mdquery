mod table;

use crate::types::Row;
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    Table,
    Json,
    Csv,
}

impl std::str::FromStr for Format {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "table" => Ok(Format::Table),
            "json" => Ok(Format::Json),
            "csv" => Ok(Format::Csv),
            _ => anyhow::bail!("unknown format: {s} (expected: table, json, csv)"),
        }
    }
}

/// Render rows in the given format to stdout.
pub fn render(rows: &[Row], columns: &[String], format: Format) -> Result<String> {
    match format {
        Format::Table => table::render(rows, columns),
        Format::Json => render_json(rows, columns),
        Format::Csv => render_csv(rows, columns),
    }
}

fn render_json(rows: &[Row], columns: &[String]) -> Result<String> {
    let json_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let mut map = serde_json::Map::new();
            for col in columns {
                let val = row.get(col).cloned().unwrap_or(crate::types::Value::Null);
                map.insert(col.clone(), serde_json::to_value(&val).unwrap());
            }
            serde_json::Value::Object(map)
        })
        .collect();

    Ok(serde_json::to_string_pretty(&json_rows)?)
}

fn render_csv(rows: &[Row], columns: &[String]) -> Result<String> {
    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record(columns)?;

    for row in rows {
        let record: Vec<String> = columns
            .iter()
            .map(|col| {
                row.get(col)
                    .map(|v| v.display())
                    .unwrap_or_else(|| "".to_string())
            })
            .collect();
        wtr.write_record(&record)?;
    }

    wtr.flush()?;
    Ok(String::from_utf8(wtr.into_inner()?)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;

    fn make_rows() -> (Vec<Row>, Vec<String>) {
        let mut rows = Vec::new();
        for (title, status) in [("Alpha", "draft"), ("Beta", "published")] {
            let mut row = Row::new();
            row.insert("title".into(), Value::String(title.into()));
            row.insert("status".into(), Value::String(status.into()));
            rows.push(row);
        }
        (rows, vec!["title".into(), "status".into()])
    }

    #[test]
    fn json_output() {
        let (rows, cols) = make_rows();
        let output = render(&rows, &cols, Format::Json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 2);
        assert_eq!(parsed[0]["title"], "Alpha");
    }

    #[test]
    fn csv_output() {
        let (rows, cols) = make_rows();
        let output = render(&rows, &cols, Format::Csv).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        assert_eq!(lines.len(), 3); // header + 2 rows
        assert!(lines[0].contains("title"));
    }

    #[test]
    fn table_output() {
        let (rows, cols) = make_rows();
        let output = render(&rows, &cols, Format::Table).unwrap();
        assert!(output.contains("Alpha"));
        assert!(output.contains("Beta"));
    }
}
