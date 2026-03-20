/// Dynamic value type for frontmatter fields and virtual columns.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Date(chrono::NaiveDate),
    DateTime(chrono::NaiveDateTime),
    Array(Vec<Value>),
}

impl Value {
    /// Convert a serde_yml::Value into our Value type.
    pub fn from_yaml(yaml: &serde_yml::Value) -> Self {
        match yaml {
            serde_yml::Value::Null => Value::Null,
            serde_yml::Value::Bool(b) => Value::Bool(*b),
            serde_yml::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Int(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::Null
                }
            }
            serde_yml::Value::String(s) => {
                // Try parsing as date first
                if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                    return Value::Date(d);
                }
                if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
                    return Value::DateTime(dt);
                }
                Value::String(s.clone())
            }
            serde_yml::Value::Sequence(seq) => {
                Value::Array(seq.iter().map(Value::from_yaml).collect())
            }
            serde_yml::Value::Mapping(_) => {
                // Flatten nested maps to JSON string for display
                Value::String(format!("{yaml:?}"))
            }
            serde_yml::Value::Tagged(tagged) => Value::from_yaml(&tagged.value),
        }
    }

    /// Returns a display-friendly string representation.
    pub fn display(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => s.clone(),
            Value::Date(d) => d.to_string(),
            Value::DateTime(dt) => dt.to_string(),
            Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| v.display()).collect();
                format!("[{}]", items.join(", "))
            }
        }
    }

    /// Check if this value contains another value (for CONTAINS operator).
    /// Works on arrays (membership) and strings (substring).
    pub fn contains(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Array(arr), val) => arr.iter().any(|item| item == val),
            (Value::String(haystack), Value::String(needle)) => haystack.contains(needle.as_str()),
            _ => false,
        }
    }

    /// Compare two values, returning an ordering if comparable.
    pub fn partial_cmp(&self, other: &Value) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
            (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Value::String(a), Value::String(b)) => a.partial_cmp(b),
            (Value::Date(a), Value::Date(b)) => a.partial_cmp(b),
            (Value::DateTime(a), Value::DateTime(b)) => a.partial_cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}

impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_none(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Int(i) => serializer.serialize_i64(*i),
            Value::Float(f) => serializer.serialize_f64(*f),
            Value::String(s) => serializer.serialize_str(s),
            Value::Date(d) => serializer.serialize_str(&d.to_string()),
            Value::DateTime(dt) => serializer.serialize_str(&dt.to_string()),
            Value::Array(arr) => arr.serialize(serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_yaml_string() {
        let yaml = serde_yml::Value::String("hello".into());
        assert_eq!(Value::from_yaml(&yaml), Value::String("hello".into()));
    }

    #[test]
    fn from_yaml_date() {
        let yaml = serde_yml::Value::String("2025-01-15".into());
        let expected = Value::Date(chrono::NaiveDate::from_ymd_opt(2025, 1, 15).unwrap());
        assert_eq!(Value::from_yaml(&yaml), expected);
    }

    #[test]
    fn from_yaml_int() {
        let yaml = serde_yml::Value::Number(serde_yml::Number::from(42));
        assert_eq!(Value::from_yaml(&yaml), Value::Int(42));
    }

    #[test]
    fn from_yaml_array() {
        let yaml = serde_yml::Value::Sequence(vec![
            serde_yml::Value::String("rust".into()),
            serde_yml::Value::String("cli".into()),
        ]);
        assert_eq!(
            Value::from_yaml(&yaml),
            Value::Array(vec![
                Value::String("rust".into()),
                Value::String("cli".into()),
            ])
        );
    }

    #[test]
    fn contains_array() {
        let arr = Value::Array(vec![
            Value::String("rust".into()),
            Value::String("cli".into()),
        ]);
        assert!(arr.contains(&Value::String("rust".into())));
        assert!(!arr.contains(&Value::String("go".into())));
    }

    #[test]
    fn contains_string() {
        let s = Value::String("hello world".into());
        assert!(s.contains(&Value::String("world".into())));
        assert!(!s.contains(&Value::String("foo".into())));
    }

    #[test]
    fn ordering() {
        let a = Value::Int(1);
        let b = Value::Int(2);
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Less));

        let a = Value::Date(chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
        let b = Value::Date(chrono::NaiveDate::from_ymd_opt(2025, 6, 1).unwrap());
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Less));
    }
}
