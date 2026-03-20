mod ast;

pub use ast::{BinOp, Expr, OrderBy, OrderDir, Query, SelectItem};

use anyhow::{Result, bail};
use sqlparser::ast::{
    BinaryOperator, Expr as SqlExpr, FunctionArg, FunctionArgExpr, FunctionArguments, GroupByExpr,
    Offset, OrderByKind, SelectItem as SqlSelectItem, SetExpr, Statement, UnaryOperator,
    Value as SqlValue,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

/// Preprocess SQL to handle mdquery-specific syntax before sqlparser sees it.
///
/// 1. Quotes the FROM path: `FROM ./path` → `FROM \`./path\``
/// 2. Rewrites CONTAINS operator: `tags CONTAINS 'x'` → `CONTAINS(tags, 'x')`
fn preprocess_sql(sql: &str) -> String {
    let mut result = rewrite_contains(sql);
    result = quote_from_path(&result);
    result
}

/// Rewrite `<expr> CONTAINS <expr>` into `CONTAINS(<expr>, <expr>)`.
/// Handles the operator form so users can write `WHERE tags CONTAINS 'rust'`.
fn rewrite_contains(sql: &str) -> String {
    let upper = sql.to_uppercase();
    let mut result = String::with_capacity(sql.len());
    let chars: Vec<char> = sql.chars().collect();
    let upper_chars: Vec<char> = upper.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Look for " CONTAINS " (with spaces around it)
        if i + 10 <= len {
            let slice: String = upper_chars[i..i + 10].iter().collect();
            if slice == " CONTAINS " {
                // Find the left operand: scan backwards from position i to find the token
                let left_end = i;
                let left_start = find_token_start(&chars, left_end);
                let left: String = chars[left_start..left_end].iter().collect();

                // Find the right operand: scan forward from position i+10
                let right_start = i + 10;
                let right_end = find_token_end(&chars, right_start);
                let right: String = chars[right_start..right_end].iter().collect();

                // Replace: rewind result to remove left operand, write function call
                let result_trimmed_len = result.len() - (left_end - left_start);
                result.truncate(result_trimmed_len);
                result.push_str(&format!("CONTAINS({left}, {right})"));
                i = right_end;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn find_token_start(chars: &[char], end: usize) -> usize {
    let mut i = end;
    // Skip trailing whitespace
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    let _token_end = i;
    if i > 0 && chars[i - 1] == '\'' {
        // Quoted string: scan back to opening quote
        i -= 1;
        while i > 0 && chars[i - 1] != '\'' {
            i -= 1;
        }
        i = i.saturating_sub(1);
    } else {
        // Identifier: scan back while alphanumeric/underscore
        while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            i -= 1;
        }
    }
    i
}

fn find_token_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    let len = chars.len();
    // Skip leading whitespace
    while i < len && chars[i].is_whitespace() {
        i += 1;
    }
    if i < len && chars[i] == '\'' {
        // Quoted string: scan to closing quote
        i += 1;
        while i < len && chars[i] != '\'' {
            i += 1;
        }
        if i < len {
            i += 1; // include closing quote
        }
    } else {
        // Identifier or number
        while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
    }
    i
}

fn quote_from_path(sql: &str) -> String {
    let upper = sql.to_uppercase();
    let mut result = String::with_capacity(sql.len() + 4);

    if let Some(from_pos) = upper.find("FROM") {
        result.push_str(&sql[..from_pos + 4]);
        let after_from = &sql[from_pos + 4..];

        let trimmed = after_from.trim_start();
        let ws_len = after_from.len() - trimmed.len();
        result.push_str(&after_from[..ws_len]);

        if !trimmed.starts_with('`') && !trimmed.starts_with('"') {
            let end = trimmed
                .find(|c: char| c.is_whitespace())
                .unwrap_or(trimmed.len());
            let path = &trimmed[..end];
            result.push('`');
            result.push_str(path);
            result.push('`');
            result.push_str(&trimmed[end..]);
        } else {
            result.push_str(trimmed);
        }
    } else {
        return sql.to_string();
    }

    result
}

/// Parse a SQL query string into our internal Query AST.
pub fn parse(sql: &str) -> Result<Query> {
    let sql = preprocess_sql(sql);
    let dialect = GenericDialect {};
    let statements = Parser::parse_sql(&dialect, &sql)?;

    if statements.len() != 1 {
        bail!("expected exactly one SQL statement");
    }

    let statement = statements.into_iter().next().unwrap();
    let Statement::Query(query) = statement else {
        bail!("expected a SELECT query");
    };

    let SetExpr::Select(select) = *query.body else {
        bail!("expected a simple SELECT (no UNION, etc.)");
    };

    // Parse DISTINCT
    let distinct = select.distinct.is_some();

    // Parse SELECT columns
    let columns = select
        .projection
        .iter()
        .map(parse_select_item)
        .collect::<Result<Vec<_>>>()?;

    // Parse FROM — we expect a single table name which is the directory path
    let from = if select.from.is_empty() {
        ".".to_string()
    } else {
        let table = &select.from[0];
        table.relation.to_string().replace(['`', '"'], "")
    };

    // Parse WHERE
    let filter = select.selection.as_ref().map(parse_expr).transpose()?;

    // Parse GROUP BY
    let group_by = match &select.group_by {
        GroupByExpr::All(_) => bail!("GROUP BY ALL is not supported"),
        GroupByExpr::Expressions(exprs, _) => {
            exprs.iter().map(parse_expr).collect::<Result<Vec<_>>>()?
        }
    };

    // Parse ORDER BY
    let order_by = if let Some(ob) = &query.order_by {
        match &ob.kind {
            OrderByKind::Expressions(exprs) => exprs
                .iter()
                .map(|item| {
                    let expr = parse_expr(&item.expr)?;
                    let dir = if item.options.asc == Some(false) {
                        OrderDir::Desc
                    } else {
                        OrderDir::Asc
                    };
                    Ok(OrderBy { expr, dir })
                })
                .collect::<Result<Vec<_>>>()?,
            OrderByKind::All(_) => bail!("ORDER BY ALL is not supported"),
        }
    } else {
        Vec::new()
    };

    // Parse LIMIT
    let limit = query.limit.as_ref().map(parse_limit).transpose()?;

    // Parse OFFSET
    let offset = query.offset.as_ref().map(parse_offset).transpose()?;

    // Determine if content is needed
    let needs_content = columns.iter().any(|c| match c {
        SelectItem::Column(name) => name == "content",
        SelectItem::AllColumns => true,
        SelectItem::Function { name, .. } => name == "MATCH" || name == "SNIPPET",
    }) || filter.as_ref().is_some_and(expr_references_content);

    // Detect aggregate queries (COUNT, etc.) — either explicit COUNT or GROUP BY present
    let is_aggregate = !group_by.is_empty()
        || columns
            .iter()
            .any(|c| matches!(c, SelectItem::Function { name, .. } if name == "COUNT"));

    Ok(Query {
        columns,
        from,
        filter,
        order_by,
        group_by,
        limit,
        offset,
        needs_content,
        is_aggregate,
        distinct,
    })
}

fn extract_function_args(args: &FunctionArguments) -> Result<Vec<Expr>> {
    match args {
        FunctionArguments::None => Ok(Vec::new()),
        FunctionArguments::Subquery(_) => bail!("subquery function arguments not supported"),
        FunctionArguments::List(list) => list
            .args
            .iter()
            .map(|arg| match arg {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => parse_expr(e),
                FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => {
                    Ok(Expr::Literal(crate::types::Value::String("*".into())))
                }
                _ => bail!("unsupported function argument"),
            })
            .collect::<Result<Vec<_>>>(),
    }
}

fn parse_select_item(item: &SqlSelectItem) -> Result<SelectItem> {
    match item {
        SqlSelectItem::UnnamedExpr(expr) => parse_select_expr(expr),
        SqlSelectItem::Wildcard(_) => Ok(SelectItem::AllColumns),
        SqlSelectItem::ExprWithAlias { expr, .. } => parse_select_expr(expr),
        _ => bail!("unsupported select item: {item}"),
    }
}

fn parse_select_expr(expr: &SqlExpr) -> Result<SelectItem> {
    match expr {
        SqlExpr::Identifier(ident) => Ok(SelectItem::Column(ident.value.clone())),
        SqlExpr::Function(func) => {
            let name = func.name.to_string().to_uppercase();
            let args = extract_function_args(&func.args)?;
            Ok(SelectItem::Function { name, args })
        }
        _ => bail!("unsupported select expression: {expr}"),
    }
}

fn parse_expr(expr: &SqlExpr) -> Result<Expr> {
    match expr {
        SqlExpr::Identifier(ident) => Ok(Expr::Column(ident.value.clone())),
        SqlExpr::Value(val) => parse_value(&val.value),
        SqlExpr::BinaryOp { left, op, right } => {
            let left = parse_expr(left)?;
            let right = parse_expr(right)?;
            let op = match op {
                BinaryOperator::Eq => ast::BinOp::Eq,
                BinaryOperator::NotEq => ast::BinOp::NotEq,
                BinaryOperator::Lt => ast::BinOp::Lt,
                BinaryOperator::LtEq => ast::BinOp::LtEq,
                BinaryOperator::Gt => ast::BinOp::Gt,
                BinaryOperator::GtEq => ast::BinOp::GtEq,
                BinaryOperator::And => ast::BinOp::And,
                BinaryOperator::Or => ast::BinOp::Or,
                _ => bail!("unsupported operator: {op}"),
            };
            Ok(Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            })
        }
        SqlExpr::UnaryOp {
            op: UnaryOperator::Not,
            expr,
        } => {
            let inner = parse_expr(expr)?;
            Ok(Expr::Not(Box::new(inner)))
        }
        SqlExpr::Like {
            negated,
            expr,
            pattern,
            ..
        } => {
            let left = parse_expr(expr)?;
            let right = parse_expr(pattern)?;
            let like = Expr::Like {
                expr: Box::new(left),
                pattern: Box::new(right),
            };
            if *negated {
                Ok(Expr::Not(Box::new(like)))
            } else {
                Ok(like)
            }
        }
        SqlExpr::InList {
            expr,
            list,
            negated,
        } => {
            let expr = parse_expr(expr)?;
            let list = list.iter().map(parse_expr).collect::<Result<Vec<_>>>()?;
            Ok(Expr::InList {
                expr: Box::new(expr),
                list,
                negated: *negated,
            })
        }
        SqlExpr::Function(func) => {
            let name = func.name.to_string().to_uppercase();
            let args = extract_function_args(&func.args)?;
            Ok(Expr::Function { name, args })
        }
        SqlExpr::Nested(inner) => parse_expr(inner),
        SqlExpr::IsNull(inner) => {
            let expr = parse_expr(inner)?;
            Ok(Expr::IsNull(Box::new(expr)))
        }
        SqlExpr::IsNotNull(inner) => {
            let expr = parse_expr(inner)?;
            Ok(Expr::Not(Box::new(Expr::IsNull(Box::new(expr)))))
        }
        _ => bail!("unsupported expression: {expr}"),
    }
}

fn parse_value(val: &SqlValue) -> Result<Expr> {
    match val {
        SqlValue::SingleQuotedString(s) | SqlValue::DoubleQuotedString(s) => {
            Ok(Expr::Literal(crate::types::Value::String(s.clone())))
        }
        SqlValue::Number(n, _) => {
            if let Ok(i) = n.parse::<i64>() {
                Ok(Expr::Literal(crate::types::Value::Int(i)))
            } else if let Ok(f) = n.parse::<f64>() {
                Ok(Expr::Literal(crate::types::Value::Float(f)))
            } else {
                bail!("invalid number: {n}")
            }
        }
        SqlValue::Boolean(b) => Ok(Expr::Literal(crate::types::Value::Bool(*b))),
        SqlValue::Null => Ok(Expr::Literal(crate::types::Value::Null)),
        _ => bail!("unsupported value: {val}"),
    }
}

fn parse_limit(expr: &SqlExpr) -> Result<usize> {
    match expr {
        SqlExpr::Value(val) => match &val.value {
            SqlValue::Number(n, _) => Ok(n.parse::<usize>()?),
            _ => bail!("LIMIT must be a number"),
        },
        _ => bail!("LIMIT must be a number"),
    }
}

fn parse_offset(offset: &Offset) -> Result<usize> {
    match &offset.value {
        SqlExpr::Value(val) => match &val.value {
            SqlValue::Number(n, _) => Ok(n.parse::<usize>()?),
            _ => bail!("OFFSET must be a number"),
        },
        _ => bail!("OFFSET must be a number"),
    }
}

fn expr_references_content(expr: &Expr) -> bool {
    match expr {
        Expr::Column(name) => name == "content",
        Expr::Function { name, .. } => name == "MATCH" || name == "SNIPPET",
        Expr::BinaryOp { left, right, .. } => {
            expr_references_content(left) || expr_references_content(right)
        }
        Expr::Not(inner) | Expr::IsNull(inner) => expr_references_content(inner),
        Expr::Like { expr, pattern } => {
            expr_references_content(expr) || expr_references_content(pattern)
        }
        Expr::InList { expr, list, .. } => {
            expr_references_content(expr) || list.iter().any(expr_references_content)
        }
        Expr::Literal(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_select() {
        let q = parse("SELECT title, date FROM .").unwrap();
        assert_eq!(q.columns.len(), 2);
        assert_eq!(q.from, ".");
        assert!(q.filter.is_none());
        assert!(!q.needs_content);
    }

    #[test]
    fn parse_select_star() {
        let q = parse("SELECT * FROM ./notes").unwrap();
        assert!(matches!(q.columns[0], SelectItem::AllColumns));
        assert_eq!(q.from, "./notes");
        assert!(q.needs_content);
    }

    #[test]
    fn parse_where_eq() {
        let q = parse("SELECT title FROM . WHERE status = 'draft'").unwrap();
        assert!(q.filter.is_some());
        let filter = q.filter.unwrap();
        assert!(matches!(
            filter,
            Expr::BinaryOp {
                op: ast::BinOp::Eq,
                ..
            }
        ));
    }

    #[test]
    fn parse_order_by() {
        let q = parse("SELECT title FROM . ORDER BY date DESC").unwrap();
        assert_eq!(q.order_by.len(), 1);
        assert!(matches!(q.order_by[0].dir, OrderDir::Desc));
    }

    #[test]
    fn parse_limit_offset() {
        let q = parse("SELECT title FROM . LIMIT 10 OFFSET 5").unwrap();
        assert_eq!(q.limit, Some(10));
        assert_eq!(q.offset, Some(5));
    }

    #[test]
    fn parse_like() {
        let q = parse("SELECT title FROM . WHERE title LIKE '%query%'").unwrap();
        assert!(matches!(q.filter, Some(Expr::Like { .. })));
    }

    #[test]
    fn parse_and_or() {
        let q = parse("SELECT title FROM . WHERE status = 'draft' AND tags = 'rust'").unwrap();
        assert!(matches!(
            q.filter,
            Some(Expr::BinaryOp {
                op: ast::BinOp::And,
                ..
            })
        ));
    }

    #[test]
    fn parse_function_in_where() {
        let q = parse("SELECT title FROM . WHERE MATCH(content, 'async runtime')").unwrap();
        assert!(q.needs_content);
    }

    #[test]
    fn content_not_needed_for_frontmatter_only() {
        let q = parse("SELECT title, tags FROM . WHERE status = 'draft'").unwrap();
        assert!(!q.needs_content);
    }

    #[test]
    fn contains_operator_rewrite() {
        let q = parse("SELECT title FROM . WHERE tags CONTAINS 'rust'").unwrap();
        assert!(matches!(
            q.filter,
            Some(Expr::Function {
                ref name,
                ..
            }) if name == "CONTAINS"
        ));
    }

    #[test]
    fn contains_operator_in_and() {
        let q =
            parse("SELECT title FROM . WHERE tags CONTAINS 'rust' AND status = 'draft'").unwrap();
        assert!(matches!(
            q.filter,
            Some(Expr::BinaryOp {
                op: ast::BinOp::And,
                ..
            })
        ));
    }

    #[test]
    fn count_star() {
        let q = parse("SELECT COUNT(*) FROM .").unwrap();
        assert!(q.is_aggregate);
        assert!(matches!(
            &q.columns[0],
            SelectItem::Function { name, .. } if name == "COUNT"
        ));
    }

    #[test]
    fn preprocess_contains_rewrite() {
        assert_eq!(
            rewrite_contains("tags CONTAINS 'rust'"),
            "CONTAINS(tags, 'rust')"
        );
    }

    #[test]
    fn preprocess_contains_in_context() {
        let result = rewrite_contains("WHERE tags CONTAINS 'rust' AND status = 'draft'");
        assert!(result.contains("CONTAINS(tags, 'rust')"));
        assert!(result.contains("AND status = 'draft'"));
    }

    #[test]
    fn parse_in_list() {
        let q = parse("SELECT title FROM . WHERE status IN ('draft', 'review')").unwrap();
        assert!(matches!(
            q.filter,
            Some(Expr::InList { negated: false, .. })
        ));
    }

    #[test]
    fn parse_not_in_list() {
        let q = parse("SELECT title FROM . WHERE status NOT IN ('archived')").unwrap();
        assert!(matches!(q.filter, Some(Expr::InList { negated: true, .. })));
    }

    #[test]
    fn parse_distinct() {
        let q = parse("SELECT DISTINCT status FROM .").unwrap();
        assert!(q.distinct);
    }

    #[test]
    fn parse_group_by() {
        let q = parse("SELECT status, COUNT(*) FROM . GROUP BY status").unwrap();
        assert_eq!(q.group_by.len(), 1);
        assert!(q.is_aggregate);
        assert!(matches!(&q.group_by[0], Expr::Column(name) if name == "status"));
    }

    #[test]
    fn non_distinct_default() {
        let q = parse("SELECT title FROM .").unwrap();
        assert!(!q.distinct);
    }
}
