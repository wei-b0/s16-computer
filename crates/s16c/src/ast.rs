//! The s16c AST — a direct transcription of LANGUAGE.md §8.

use crate::Pos;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scalar {
    U16,
    I16,
    Bool,
}

impl std::fmt::Display for Scalar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Scalar::U16 => "u16",
            Scalar::I16 => "i16",
            Scalar::Bool => "bool",
        })
    }
}

#[derive(Debug)]
pub enum Item {
    Fn(FnItem),
    Static(StaticItem),
    Const(ConstItem),
    Use(UseItem),
}

/// `use screen;` — pulls in a named standard library (LANGUAGE.md §7).
#[derive(Debug)]
pub struct UseItem {
    pub name: String,
    pub pos: Pos,
}

#[derive(Debug)]
pub struct FnItem {
    pub name: String,
    pub pos: Pos,
    pub params: Vec<(String, Scalar, Pos)>,
    pub ret: Option<Scalar>,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct StaticItem {
    pub name: String,
    pub pos: Pos,
    /// element type and, for arrays, the length expression
    pub elem: Scalar,
    pub len: Option<Expr>,
    pub init: Init,
}

#[derive(Debug)]
pub enum Init {
    Scalar(Expr),
    Repeat(Expr, Expr), // [value; count]
    List(Vec<Expr>),    // [a, b, c]
}

#[derive(Debug)]
pub struct ConstItem {
    pub name: String,
    pub pos: Pos,
    pub ty: Scalar,
    pub value: Expr,
}

#[derive(Debug)]
pub enum Stmt {
    Let {
        name: String,
        ty: Option<Scalar>,
        value: Expr,
    },
    Assign {
        name: String,
        index: Option<Expr>,
        value: Expr,
        pos: Pos,
    },
    If {
        cond: Expr,
        then: Vec<Stmt>,
        els: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Loop {
        body: Vec<Stmt>,
    },
    Break(Pos),
    Continue(Pos),
    Return(Option<Expr>, Pos),
    Expr(Expr),
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub pos: Pos,
}

#[derive(Debug)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    Name(String),
    Index(String, Box<Expr>),
    Call(String, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Cast(Box<Expr>, Scalar),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    LogAnd,
    LogOr,
}

impl BinOp {
    pub fn is_compare(self) -> bool {
        matches!(self, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
    }
}
