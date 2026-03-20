mod value;

pub use value::Value;

use std::collections::BTreeMap;

/// A single row representing one markdown file's queryable data.
#[derive(Debug, Clone, Default)]
pub struct Row {
    /// Frontmatter fields + virtual columns, keyed by column name.
    pub fields: BTreeMap<String, Value>,
}

impl Row {
    pub fn new() -> Self {
        Self {
            fields: BTreeMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }

    pub fn insert(&mut self, key: String, value: Value) {
        self.fields.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_insert_and_get() {
        let mut row = Row::new();
        row.insert("title".into(), Value::String("Hello".into()));
        assert_eq!(row.get("title"), Some(&Value::String("Hello".into())));
        assert_eq!(row.get("missing"), None);
    }
}
