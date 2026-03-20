use crate::parser::{BinOp, Expr};
use crate::types::{Row, Value};

/// Evaluate a WHERE expression against a row, returning true if the row passes.
pub fn eval_expr(expr: &Expr, row: &Row) -> bool {
    match eval_to_value(expr, row) {
        Value::Bool(b) => b,
        Value::Null => false,
        _ => true, // truthy: any non-null, non-bool value is true
    }
}

/// Public wrapper for eval_to_value, used by projection in eval/mod.rs.
pub fn eval_to_value_pub(expr: &Expr, row: &Row) -> Value {
    eval_to_value(expr, row)
}

/// Public wrapper for eval_function, used by projection in eval/mod.rs.
pub fn eval_function_pub(name: &str, args: &[Expr], row: &Row) -> Value {
    eval_function(name, args, row)
}

fn eval_to_value(expr: &Expr, row: &Row) -> Value {
    match expr {
        Expr::Column(name) => row.get(name).cloned().unwrap_or(Value::Null),
        Expr::Literal(val) => val.clone(),
        Expr::BinaryOp { left, op, right } => {
            let left_val = eval_to_value(left, row);
            let right_val = eval_to_value(right, row);

            match op {
                BinOp::And => Value::Bool(is_truthy(&left_val) && is_truthy(&right_val)),
                BinOp::Or => Value::Bool(is_truthy(&left_val) || is_truthy(&right_val)),
                BinOp::Eq => Value::Bool(left_val == right_val),
                BinOp::NotEq => Value::Bool(left_val != right_val),
                BinOp::Lt => Value::Bool(
                    left_val
                        .partial_cmp(&right_val)
                        .is_some_and(|o| o == std::cmp::Ordering::Less),
                ),
                BinOp::LtEq => Value::Bool(
                    left_val
                        .partial_cmp(&right_val)
                        .is_some_and(|o| o != std::cmp::Ordering::Greater),
                ),
                BinOp::Gt => Value::Bool(
                    left_val
                        .partial_cmp(&right_val)
                        .is_some_and(|o| o == std::cmp::Ordering::Greater),
                ),
                BinOp::GtEq => Value::Bool(
                    left_val
                        .partial_cmp(&right_val)
                        .is_some_and(|o| o != std::cmp::Ordering::Less),
                ),
            }
        }
        Expr::Not(inner) => Value::Bool(!is_truthy(&eval_to_value(inner, row))),
        Expr::Like { expr, pattern } => {
            let val = eval_to_value(expr, row);
            let pat = eval_to_value(pattern, row);
            match (&val, &pat) {
                (Value::String(s), Value::String(p)) => Value::Bool(like_match(s, p)),
                _ => Value::Bool(false),
            }
        }
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let val = eval_to_value(expr, row);
            let matched = list.iter().any(|item| eval_to_value(item, row) == val);
            Value::Bool(if *negated { !matched } else { matched })
        }
        Expr::IsNull(inner) => {
            let val = eval_to_value(inner, row);
            Value::Bool(matches!(val, Value::Null))
        }
        Expr::Function { name, args } => eval_function(name, args, row),
    }
}

fn eval_function(name: &str, args: &[Expr], row: &Row) -> Value {
    match name {
        "MATCH" => {
            if args.len() != 2 {
                return Value::Bool(false);
            }
            let haystack = eval_to_value(&args[0], row);
            let needle = eval_to_value(&args[1], row);
            match (&haystack, &needle) {
                (Value::String(text), Value::String(query)) => {
                    Value::Bool(fuzzy_match(text, query))
                }
                _ => Value::Bool(false),
            }
        }
        "CONTAINS" => {
            if args.len() != 2 {
                return Value::Bool(false);
            }
            let haystack = eval_to_value(&args[0], row);
            let needle = eval_to_value(&args[1], row);
            Value::Bool(haystack.contains(&needle))
        }
        "NOW" => Value::Date(chrono::Local::now().date_naive()),
        "DATE_SUB" => {
            // DATE_SUB(date, days) — subtract N days from a date
            if args.len() != 2 {
                return Value::Null;
            }
            let date_val = eval_to_value(&args[0], row);
            let days_val = eval_to_value(&args[1], row);
            match (&date_val, &days_val) {
                (Value::Date(d), Value::Int(days)) => {
                    Value::Date(*d - chrono::Days::new(*days as u64))
                }
                _ => Value::Null,
            }
        }
        "DATE_ADD" => {
            // DATE_ADD(date, days) — add N days to a date
            if args.len() != 2 {
                return Value::Null;
            }
            let date_val = eval_to_value(&args[0], row);
            let days_val = eval_to_value(&args[1], row);
            match (&date_val, &days_val) {
                (Value::Date(d), Value::Int(days)) => {
                    Value::Date(*d + chrono::Days::new(*days as u64))
                }
                _ => Value::Null,
            }
        }
        "DATE" => {
            if args.len() != 1 {
                return Value::Null;
            }
            let val = eval_to_value(&args[0], row);
            match val {
                Value::String(s) => {
                    if let Ok(d) = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                        Value::Date(d)
                    } else {
                        Value::Null
                    }
                }
                Value::Date(_) => val,
                _ => Value::Null,
            }
        }
        "LOWER" => {
            if args.len() != 1 {
                return Value::Null;
            }
            let val = eval_to_value(&args[0], row);
            match val {
                Value::String(s) => Value::String(s.to_lowercase()),
                _ => val,
            }
        }
        "UPPER" => {
            if args.len() != 1 {
                return Value::Null;
            }
            let val = eval_to_value(&args[0], row);
            match val {
                Value::String(s) => Value::String(s.to_uppercase()),
                _ => val,
            }
        }
        "LEN" => {
            if args.len() != 1 {
                return Value::Null;
            }
            let val = eval_to_value(&args[0], row);
            match val {
                Value::String(s) => Value::Int(s.len() as i64),
                Value::Array(arr) => Value::Int(arr.len() as i64),
                _ => Value::Null,
            }
        }
        _ => Value::Null,
    }
}

/// Fuzzy match using nucleo-matcher. Returns true if the query fuzzy-matches the text.
/// Falls back to case-insensitive substring match if nucleo gives no match.
fn fuzzy_match(text: &str, query: &str) -> bool {
    use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
    use nucleo_matcher::{Config, Matcher};

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut buf = Vec::new();
    let haystack = nucleo_matcher::Utf32Str::new(text, &mut buf);
    pattern.score(haystack, &mut matcher).is_some()
}

fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Null => false,
        Value::Bool(b) => *b,
        _ => true,
    }
}

/// SQL LIKE pattern matching (% = any chars, _ = single char).
fn like_match(value: &str, pattern: &str) -> bool {
    let value = value.to_lowercase();
    let pattern = pattern.to_lowercase();

    let v: Vec<char> = value.chars().collect();
    let p: Vec<char> = pattern.chars().collect();

    like_match_inner(&v, &p, 0, 0)
}

fn like_match_inner(value: &[char], pattern: &[char], vi: usize, pi: usize) -> bool {
    if pi == pattern.len() {
        return vi == value.len();
    }

    match pattern[pi] {
        '%' => {
            // % matches zero or more characters
            for i in vi..=value.len() {
                if like_match_inner(value, pattern, i, pi + 1) {
                    return true;
                }
            }
            false
        }
        '_' => {
            // _ matches exactly one character
            if vi < value.len() {
                like_match_inner(value, pattern, vi + 1, pi + 1)
            } else {
                false
            }
        }
        ch => {
            if vi < value.len() && value[vi] == ch {
                like_match_inner(value, pattern, vi + 1, pi + 1)
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{BinOp, Expr};

    fn make_row() -> Row {
        let mut row = Row::new();
        row.insert("title".into(), Value::String("Hello World".into()));
        row.insert("status".into(), Value::String("draft".into()));
        row.insert("count".into(), Value::Int(42));
        row.insert(
            "tags".into(),
            Value::Array(vec![
                Value::String("rust".into()),
                Value::String("cli".into()),
            ]),
        );
        row.insert(
            "date".into(),
            Value::Date(chrono::NaiveDate::from_ymd_opt(2025, 3, 15).unwrap()),
        );
        row
    }

    #[test]
    fn eq_string() {
        let row = make_row();
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Column("status".into())),
            op: BinOp::Eq,
            right: Box::new(Expr::Literal(Value::String("draft".into()))),
        };
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn eq_string_mismatch() {
        let row = make_row();
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Column("status".into())),
            op: BinOp::Eq,
            right: Box::new(Expr::Literal(Value::String("published".into()))),
        };
        assert!(!eval_expr(&expr, &row));
    }

    #[test]
    fn not_eq() {
        let row = make_row();
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Column("status".into())),
            op: BinOp::NotEq,
            right: Box::new(Expr::Literal(Value::String("published".into()))),
        };
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn gt_int() {
        let row = make_row();
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Column("count".into())),
            op: BinOp::Gt,
            right: Box::new(Expr::Literal(Value::Int(10))),
        };
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn and_expr() {
        let row = make_row();
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Column("status".into())),
                op: BinOp::Eq,
                right: Box::new(Expr::Literal(Value::String("draft".into()))),
            }),
            op: BinOp::And,
            right: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Column("count".into())),
                op: BinOp::Gt,
                right: Box::new(Expr::Literal(Value::Int(10))),
            }),
        };
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn or_expr() {
        let row = make_row();
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Column("status".into())),
                op: BinOp::Eq,
                right: Box::new(Expr::Literal(Value::String("published".into()))),
            }),
            op: BinOp::Or,
            right: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Column("count".into())),
                op: BinOp::Gt,
                right: Box::new(Expr::Literal(Value::Int(10))),
            }),
        };
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn not_expr() {
        let row = make_row();
        let expr = Expr::Not(Box::new(Expr::BinaryOp {
            left: Box::new(Expr::Column("status".into())),
            op: BinOp::Eq,
            right: Box::new(Expr::Literal(Value::String("published".into()))),
        }));
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn like_pattern() {
        let row = make_row();
        let expr = Expr::Like {
            expr: Box::new(Expr::Column("title".into())),
            pattern: Box::new(Expr::Literal(Value::String("%world".into()))),
        };
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn like_pattern_no_match() {
        let row = make_row();
        let expr = Expr::Like {
            expr: Box::new(Expr::Column("title".into())),
            pattern: Box::new(Expr::Literal(Value::String("%foo%".into()))),
        };
        assert!(!eval_expr(&expr, &row));
    }

    #[test]
    fn is_null_missing_column() {
        let row = make_row();
        let expr = Expr::IsNull(Box::new(Expr::Column("nonexistent".into())));
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn contains_function_array() {
        let row = make_row();
        let expr = Expr::Function {
            name: "CONTAINS".into(),
            args: vec![
                Expr::Column("tags".into()),
                Expr::Literal(Value::String("rust".into())),
            ],
        };
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn contains_function_string() {
        let row = make_row();
        let expr = Expr::Function {
            name: "CONTAINS".into(),
            args: vec![
                Expr::Column("title".into()),
                Expr::Literal(Value::String("World".into())),
            ],
        };
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn like_match_patterns() {
        assert!(like_match("hello", "%"));
        assert!(like_match("hello", "hello"));
        assert!(like_match("hello", "%llo"));
        assert!(like_match("hello", "hel%"));
        assert!(like_match("hello", "%ell%"));
        assert!(like_match("hello", "h_llo"));
        assert!(!like_match("hello", "h_lo"));
    }

    #[test]
    fn now_returns_today() {
        let row = make_row();
        let expr = Expr::Function {
            name: "NOW".into(),
            args: vec![],
        };
        let val = eval_to_value(&expr, &row);
        let today = chrono::Local::now().date_naive();
        assert_eq!(val, Value::Date(today));
    }

    #[test]
    fn date_sub() {
        let row = make_row();
        let expr = Expr::Function {
            name: "DATE_SUB".into(),
            args: vec![
                Expr::Function {
                    name: "NOW".into(),
                    args: vec![],
                },
                Expr::Literal(Value::Int(7)),
            ],
        };
        let val = eval_to_value(&expr, &row);
        let expected = chrono::Local::now().date_naive() - chrono::Days::new(7);
        assert_eq!(val, Value::Date(expected));
    }

    #[test]
    fn date_add() {
        let row = make_row();
        let expr = Expr::Function {
            name: "DATE_ADD".into(),
            args: vec![
                Expr::Function {
                    name: "NOW".into(),
                    args: vec![],
                },
                Expr::Literal(Value::Int(30)),
            ],
        };
        let val = eval_to_value(&expr, &row);
        let expected = chrono::Local::now().date_naive() + chrono::Days::new(30);
        assert_eq!(val, Value::Date(expected));
    }

    #[test]
    fn date_comparison_with_now() {
        let row = make_row();
        // row.date is 2025-03-15, NOW() is 2026-03-19 — date should be < NOW()
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Column("date".into())),
            op: BinOp::Lt,
            right: Box::new(Expr::Function {
                name: "NOW".into(),
                args: vec![],
            }),
        };
        assert!(eval_expr(&expr, &row));
    }

    #[test]
    fn date_sub_in_comparison() {
        let row = make_row();
        // row.date is 2025-03-15, DATE_SUB(NOW(), 30) is ~2026-02-17 — date < that
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Column("date".into())),
            op: BinOp::Lt,
            right: Box::new(Expr::Function {
                name: "DATE_SUB".into(),
                args: vec![
                    Expr::Function {
                        name: "NOW".into(),
                        args: vec![],
                    },
                    Expr::Literal(Value::Int(30)),
                ],
            }),
        };
        assert!(eval_expr(&expr, &row));
    }
}
