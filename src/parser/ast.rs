use crate::types::Value;

/// A parsed mdquery query.
#[derive(Debug, Clone)]
pub struct Query {
    pub columns: Vec<SelectItem>,
    pub from: String,
    pub filter: Option<Expr>,
    pub order_by: Vec<OrderBy>,
    pub group_by: Vec<Expr>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub needs_content: bool,
    pub is_aggregate: bool,
    pub distinct: bool,
}

/// A column in the SELECT clause.
#[derive(Debug, Clone)]
pub enum SelectItem {
    Column(String),
    AllColumns,
    Function { name: String, args: Vec<Expr> },
}

/// An expression in WHERE, ORDER BY, or function arguments.
#[derive(Debug, Clone)]
pub enum Expr {
    Column(String),
    Literal(Value),
    BinaryOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    Not(Box<Expr>),
    Like {
        expr: Box<Expr>,
        pattern: Box<Expr>,
    },
    InList {
        expr: Box<Expr>,
        list: Vec<Expr>,
        negated: bool,
    },
    IsNull(Box<Expr>),
    Function {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone)]
pub struct OrderBy {
    pub expr: Expr,
    pub dir: OrderDir,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrderDir {
    Asc,
    Desc,
}
