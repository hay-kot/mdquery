# mdquery

A fast CLI for SQL-like queries on markdown frontmatter. Treats a folder of markdown files as a queryable database.

## Install

Download a binary from [Releases](https://github.com/hay-kot/mdquery/releases), or build from source:

```bash
cargo install --path .
```

## Usage

```bash
mdquery "SELECT title, date FROM ./notes WHERE status = 'published' ORDER BY date DESC"
```

### Output Formats

```bash
# Table (default)
mdquery "SELECT title FROM . WHERE status = 'draft'"

# JSON (great for scripting and LLM tool use)
mdquery "SELECT path FROM . WHERE tags CONTAINS 'rust'" --format json

# CSV
mdquery "SELECT title, date FROM . ORDER BY date DESC" --format csv
```

## Query Syntax

Standard SQL SELECT with markdown-specific extensions.

### Basic Queries

```sql
-- Select specific fields
SELECT title, date, tags FROM ./notes

-- All fields (includes content)
SELECT * FROM .

-- Filter with WHERE
SELECT title FROM . WHERE status = 'draft'

-- Sort, limit, offset
SELECT title FROM . ORDER BY date DESC LIMIT 10 OFFSET 5
```

### Operators

```sql
-- Comparison: =, !=, <, >, <=, >=
SELECT title FROM . WHERE date > DATE('2025-01-01')

-- Boolean: AND, OR, NOT
SELECT title FROM . WHERE status = 'draft' AND tags CONTAINS 'rust'

-- Pattern matching
SELECT title FROM . WHERE title LIKE '%query%'

-- List membership
SELECT title FROM . WHERE status IN ('draft', 'review')
SELECT title FROM . WHERE status NOT IN ('archived')

-- Array/string containment
SELECT title FROM . WHERE tags CONTAINS 'rust'

-- Null checks
SELECT title FROM . WHERE description IS NULL
SELECT title FROM . WHERE description IS NOT NULL
```

### Functions

| Function | Description |
|---|---|
| `MATCH(content, 'query')` | Fuzzy content search (nucleo) |
| `SNIPPET(content, 'query')` | Content excerpt around match |
| `CONTAINS(field, 'value')` | Array membership or substring |
| `NOW()` | Today's date |
| `DATE('YYYY-MM-DD')` | Date literal |
| `DATE_SUB(date, days)` | Subtract days from date |
| `DATE_ADD(date, days)` | Add days to date |
| `LOWER(field)` | Lowercase |
| `UPPER(field)` | Uppercase |
| `LEN(field)` | String/array length |
| `COUNT(*)` | Count rows |

### Aggregates

```sql
-- Count all files
SELECT COUNT(*) FROM ./notes

-- Count with filter
SELECT COUNT(*) FROM . WHERE status = 'published'

-- Group by with count
SELECT status, COUNT(*) FROM . GROUP BY status ORDER BY status
```

### Special Features

```sql
-- Deduplicate results
SELECT DISTINCT status FROM .

-- Date arithmetic
SELECT path FROM . WHERE modified < DATE_SUB(NOW(), 30)
SELECT path FROM . WHERE modified > DATE_SUB(NOW(), 7)

-- Fuzzy content search
SELECT title, SNIPPET(content, 'error handling') FROM . WHERE MATCH(content, 'error handling')
```

### Virtual Columns

Available on every file without being in frontmatter:

| Column | Type | Description |
|---|---|---|
| `path` | string | Relative file path |
| `filename` | string | File name without extension |
| `content` | string | Full markdown body |
| `size` | int | File size in bytes |
| `modified` | date | Last modification date |

## Performance

- **Parallel file processing** via rayon
- **Smart reading**: frontmatter-only queries skip file content entirely
- **Fast frontmatter parser**: line-by-line reader stops at second `---`
- **.gitignore aware**: respects ignore rules via the `ignore` crate

Typical performance on ~165 files:
- Frontmatter-only query: **~15ms**
- Full content + fuzzy search: **~100ms**

## LLM Tool Use

mdquery works well as an LLM tool for managing markdown knowledge bases:

```bash
# Prune old documents
mdquery "SELECT path FROM ./docs WHERE modified < DATE_SUB(NOW(), 30)" --format json

# Search by tags
mdquery "SELECT path FROM . WHERE tags CONTAINS 'rust'" --format json

# Keyword search in recent docs
mdquery "SELECT path FROM . WHERE MATCH(content, 'error handling') AND modified > DATE_SUB(NOW(), 7)" --format json

# Find docs by status
mdquery "SELECT path FROM . WHERE status = 'ready'" --format json
```

## License

MIT
