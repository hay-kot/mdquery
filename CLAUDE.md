# mdquery

A fast Rust CLI for SQL-like queries on markdown frontmatter.

## Build & Test

```bash
cargo build
cargo test
```

## Architecture

```
src/
├── main.rs           # CLI entry point (clap)
├── lib.rs            # Public API: parse → execute → render
├── parser/           # SQL parsing (sqlparser) → internal AST
│   ├── mod.rs        # Preprocessing (CONTAINS rewrite, FROM path quoting) + parse
│   └── ast.rs        # Query, Expr, SelectItem types
├── scanner/          # File discovery + reading
│   ├── mod.rs        # find_markdown_files (ignore crate, recursive)
│   └── reader.rs     # Smart reading: frontmatter-only fast path vs full read
├── eval/             # Query execution engine
│   ├── mod.rs        # execute: filter → aggregate/sort → project → distinct
│   ├── filter.rs     # WHERE evaluation, functions (MATCH, CONTAINS, NOW, DATE_SUB, etc.)
│   └── sort.rs       # ORDER BY
├── output/           # Result formatting
│   ├── mod.rs        # Format dispatch (table/json/csv)
│   └── table.rs      # comfy-table rendering
└── types/            # Value types
    ├── mod.rs        # Row (BTreeMap<String, Value>)
    └── value.rs      # Value enum, YAML conversion, comparison, serialization
```

## Key Design Decisions

- **Smart reading**: frontmatter-only queries use a line-by-line BufReader that stops at the second `---`. Content queries use gray_matter for full parsing.
- **SQL preprocessing**: `CONTAINS` operator and FROM paths are rewritten before sqlparser sees them.
- **Parallel execution**: rayon parallelizes file reading + filtering.
- **No persistent index**: all queries scan on demand.

## Query Syntax Extensions

Beyond standard SQL:
- `CONTAINS` operator: `WHERE tags CONTAINS 'rust'` (arrays + strings)
- `MATCH(content, 'query')`: fuzzy search via nucleo-matcher
- `NOW()`, `DATE_SUB(date, days)`, `DATE_ADD(date, days)`: date arithmetic
- Virtual columns: `path`, `filename`, `content`, `size`, `modified`
