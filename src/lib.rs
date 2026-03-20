pub mod eval;
pub mod output;
pub mod parser;
pub mod scanner;
pub mod types;

use anyhow::Result;
use output::Format;
use std::path::Path;

/// Run a SQL query against markdown files in the given directory.
pub fn run(sql: &str, format: Format) -> Result<String> {
    let query = parser::parse(sql)?;

    // Resolve the FROM path
    let base_dir = Path::new(&query.from);
    let base_dir = if base_dir.is_absolute() {
        base_dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(base_dir)
    };

    if !base_dir.is_dir() {
        anyhow::bail!("FROM path is not a directory: {}", query.from);
    }

    let rows = eval::execute(&query, &base_dir)?;
    let columns = eval::column_names(&query, rows.first());

    output::render(&rows, &columns, format)
}
