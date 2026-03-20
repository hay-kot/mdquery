use crate::parser::{Expr, OrderBy, OrderDir};
use crate::types::{Row, Value};

/// Sort rows by the ORDER BY clauses.
pub fn sort_rows(rows: &mut [Row], order_by: &[OrderBy]) {
    rows.sort_by(|a, b| {
        for ob in order_by {
            let a_val = resolve_expr(&ob.expr, a);
            let b_val = resolve_expr(&ob.expr, b);

            let cmp = a_val
                .partial_cmp(&b_val)
                .unwrap_or(std::cmp::Ordering::Equal);

            let cmp = match ob.dir {
                OrderDir::Asc => cmp,
                OrderDir::Desc => cmp.reverse(),
            };

            if cmp != std::cmp::Ordering::Equal {
                return cmp;
            }
        }
        std::cmp::Ordering::Equal
    });
}

fn resolve_expr(expr: &Expr, row: &Row) -> Value {
    match expr {
        Expr::Column(name) => row.get(name).cloned().unwrap_or(Value::Null),
        Expr::Literal(val) => val.clone(),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rows() -> Vec<Row> {
        let mut rows = Vec::new();
        for (title, date) in [
            ("Gamma", "2025-03-10"),
            ("Alpha", "2025-01-01"),
            ("Beta", "2025-06-15"),
        ] {
            let mut row = Row::new();
            row.insert("title".into(), Value::String(title.into()));
            row.insert(
                "date".into(),
                Value::Date(chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap()),
            );
            rows.push(row);
        }
        rows
    }

    #[test]
    fn sort_by_string_asc() {
        let mut rows = make_rows();
        let order = vec![OrderBy {
            expr: Expr::Column("title".into()),
            dir: OrderDir::Asc,
        }];
        sort_rows(&mut rows, &order);
        let titles: Vec<_> = rows
            .iter()
            .map(|r| r.get("title").unwrap().display())
            .collect();
        assert_eq!(titles, vec!["Alpha", "Beta", "Gamma"]);
    }

    #[test]
    fn sort_by_date_desc() {
        let mut rows = make_rows();
        let order = vec![OrderBy {
            expr: Expr::Column("date".into()),
            dir: OrderDir::Desc,
        }];
        sort_rows(&mut rows, &order);
        let titles: Vec<_> = rows
            .iter()
            .map(|r| r.get("title").unwrap().display())
            .collect();
        assert_eq!(titles, vec!["Beta", "Gamma", "Alpha"]);
    }
}
