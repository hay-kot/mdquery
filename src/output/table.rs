use crate::types::Row;
use anyhow::Result;
use comfy_table::{Table, presets::UTF8_FULL_CONDENSED};

pub fn render(rows: &[Row], columns: &[String]) -> Result<String> {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(columns);

    for row in rows {
        let cells: Vec<String> = columns
            .iter()
            .map(|col| {
                row.get(col)
                    .map(|v| v.display())
                    .unwrap_or_else(|| "null".to_string())
            })
            .collect();
        table.add_row(cells);
    }

    Ok(table.to_string())
}
