//! Typechecker and const folder: AST in, typed IR (`Program`) out.
//!
//! Enforces LANGUAGE.md's rules — no u16/i16 mixing, bool conditions,
//! signedness of `/ % -`, const-only initializers — and lowers names to slots
//! (locals) and labels (statics) so codegen is mechanical.

use crate::ast::*;
use crate::{CompileError, Pos};
use std::collections::HashMap;

// --- Typed IR ---------------------------------------------------------------

pub struct Program {
    pub statics: Vec<StaticDef>,
    pub fns: Vec<TFn>,
}

pub struct StaticDef {
    pub name: String,
    pub words: Vec<u16>,
}

pub struct TFn {
    pub name: String,
    pub nparams: usize,
    /// Total frame slots: params, then lets, then call-arg staging.
    pub nslots: usize,
    /// First slot of the call-arg staging area.
    pub arg_stage: usize,
    pub body: Vec<TStmt>,
}

pub enum TStmt {
    Store { slot: usize, value: TExpr },
    StoreStatic { name: String, value: TExpr },
    StoreIndex { name: String, index: TExpr, value: TExpr },
    If { cond: TExpr, then: Vec<TStmt>, els: Vec<TStmt> },
    While { cond: TExpr, body: Vec<TStmt> },
    Loop { body: Vec<TStmt> },
    Break,
    Continue,
    Return(Option<TExpr>),
    Expr(TExpr),
}

pub struct TExpr {
    /// None = unit (a call to a function with no return type).
    pub ty: Option<Scalar>,
    pub kind: TExprKind,
}

pub enum TExprKind {
    Int(u16),
    Local(usize),
    Static(String),
    Index(String, Box<TExpr>),
    Call(String, Vec<TExpr>),
    Builtin(Builtin, Vec<TExpr>),
    Unary(UnOp, Box<TExpr>),
    /// `operands` is the shared operand type — what decides signed vs
    /// unsigned branches/shifts. For arithmetic it equals the result type.
    Binary { op: BinOp, operands: Scalar, lhs: Box<TExpr>, rhs: Box<TExpr> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Peek,
    Poke,
    Peek8,
    Poke8,
    Halt,
    Brk,
}

const BUILTINS: &[(&str, Builtin)] = &[
    ("peek", Builtin::Peek),
    ("poke", Builtin::Poke),
    ("peek8", Builtin::Peek8),
    ("poke8", Builtin::Poke8),
    ("halt", Builtin::Halt),
    ("brk", Builtin::Brk),
];

// --- Environments -----------------------------------------------------------

struct StaticInfo {
    elem: Scalar,
    len: Option<u16>, // Some = array
}

struct Globals {
    consts: HashMap<String, (Scalar, u16)>,
    statics: HashMap<String, StaticInfo>,
    fns: HashMap<String, (Vec<Scalar>, Option<Scalar>)>,
}

impl Globals {
    fn defined(&self, name: &str) -> bool {
        self.consts.contains_key(name)
            || self.statics.contains_key(name)
            || self.fns.contains_key(name)
            || BUILTINS.iter().any(|(b, _)| *b == name)
    }
}

fn err(pos: Pos, msg: String) -> CompileError {
    CompileError::at(pos, msg)
}

// --- Entry ------------------------------------------------------------------

pub fn check(items: &[Item]) -> Result<Program, CompileError> {
    let mut globals = Globals {
        consts: HashMap::new(),
        statics: HashMap::new(),
        fns: HashMap::new(),
    };
    let mut static_defs = Vec::new();

    // Pass A: consts and statics (in order, so consts can feed later items),
    // and function signatures.
    for item in items {
        match item {
            Item::Const(c) => {
                if globals.defined(&c.name) {
                    return Err(err(c.pos, format!("`{}` is already defined", c.name)));
                }
                let value = const_eval(&c.value, &globals.consts, Some(c.ty))?;
                globals.consts.insert(c.name.clone(), (c.ty, value));
            }
            Item::Static(s) => {
                if globals.defined(&s.name) {
                    return Err(err(s.pos, format!("`{}` is already defined", s.name)));
                }
                let (info, words) = check_static(s, &globals.consts)?;
                globals.statics.insert(s.name.clone(), info);
                static_defs.push(StaticDef { name: s.name.clone(), words });
            }
            Item::Fn(f) => {
                if globals.defined(&f.name) {
                    return Err(err(f.pos, format!("`{}` is already defined", f.name)));
                }
                let params = f.params.iter().map(|(_, ty, _)| *ty).collect();
                globals.fns.insert(f.name.clone(), (params, f.ret));
            }
        }
    }

    match globals.fns.get("main") {
        None => {
            return Err(err(Pos { line: 1, col: 1 }, "missing `fn main()`".into()));
        }
        Some((params, ret)) if !params.is_empty() || ret.is_some() => {
            let pos = items
                .iter()
                .find_map(|i| match i {
                    Item::Fn(f) if f.name == "main" => Some(f.pos),
                    _ => None,
                })
                .unwrap_or(Pos { line: 1, col: 1 });
            return Err(err(pos, "`fn main()` must take no parameters and return nothing".into()));
        }
        _ => {}
    }

    // Pass B: function bodies.
    let mut fns = Vec::new();
    for item in items {
        if let Item::Fn(f) = item {
            fns.push(check_fn(f, &globals)?);
        }
    }

    Ok(Program { statics: static_defs, fns })
}

// --- Constant expressions ---------------------------------------------------

fn const_eval(
    e: &Expr,
    consts: &HashMap<String, (Scalar, u16)>,
    expected: Option<Scalar>,
) -> Result<u16, CompileError> {
    let (ty, v) = const_eval_ty(e, consts, expected)?;
    if let Some(want) = expected {
        if ty != want {
            return Err(err(e.pos, format!("expected `{want}`, found `{ty}`")));
        }
    }
    Ok(v)
}

/// Literal adaptation (LANGUAGE.md §3): adopt the expected integer type when
/// the value fits; default to u16.
fn literal(pos: Pos, v: i64, expected: Option<Scalar>) -> Result<(Scalar, u16), CompileError> {
    let ty = match expected {
        Some(Scalar::I16) => Scalar::I16,
        _ => Scalar::U16,
    };
    let fits = match ty {
        Scalar::U16 => (0..=0xFFFF).contains(&v),
        Scalar::I16 => (-32768..=32767).contains(&v),
        Scalar::Bool => false,
    };
    if !fits {
        return Err(err(pos, format!("literal `{v}` out of range for `{ty}`")));
    }
    Ok((ty, v as u16))
}

fn const_eval_ty(
    e: &Expr,
    consts: &HashMap<String, (Scalar, u16)>,
    expected: Option<Scalar>,
) -> Result<(Scalar, u16), CompileError> {
    match &e.kind {
        ExprKind::Int(v) => literal(e.pos, *v, expected),
        ExprKind::Bool(b) => Ok((Scalar::Bool, *b as u16)),
        ExprKind::Name(name) => consts
            .get(name)
            .copied()
            .ok_or_else(|| err(e.pos, format!("`{name}` is not a constant"))),
        ExprKind::Unary(UnOp::Neg, inner) => {
            if let ExprKind::Int(v) = inner.kind {
                return literal(e.pos, -v, Some(Scalar::I16));
            }
            let (ty, v) = const_eval_ty(inner, consts, Some(Scalar::I16))?;
            if ty != Scalar::I16 {
                return Err(err(e.pos, "unary `-` needs an `i16` operand".into()));
            }
            Ok((Scalar::I16, (v as i16).wrapping_neg() as u16))
        }
        ExprKind::Unary(UnOp::Not, inner) => {
            let (ty, v) = const_eval_ty(inner, consts, expected)?;
            match ty {
                Scalar::Bool => Ok((Scalar::Bool, (v == 0) as u16)),
                _ => Ok((ty, !v)),
            }
        }
        ExprKind::Cast(inner, to) => {
            let (ty, v) = const_eval_ty(inner, consts, None)?;
            if ty == Scalar::Bool {
                return Err(err(e.pos, "cannot cast `bool`".into()));
            }
            Ok((*to, v))
        }
        ExprKind::Binary(op, l, r)
            if !op.is_compare() && !matches!(op, BinOp::LogAnd | BinOp::LogOr) =>
        {
            let want = expected.filter(|t| *t != Scalar::Bool);
            let (lt, lv) = const_eval_ty(l, consts, want)?;
            let (rt, rv) = const_eval_ty(r, consts, Some(lt))?;
            if lt != rt {
                return Err(err(e.pos, format!("mismatched types: `{lt}` and `{rt}`")));
            }
            if lt == Scalar::Bool {
                return Err(err(e.pos, "integer operator on `bool`".into()));
            }
            if matches!(op, BinOp::Div | BinOp::Mod) {
                if lt != Scalar::U16 {
                    return Err(err(e.pos, "`/` and `%` are `u16`-only in v1 (D018)".into()));
                }
                if rv == 0 {
                    return Err(err(e.pos, "division by zero in constant expression".into()));
                }
            }
            let v = match op {
                BinOp::Add => lv.wrapping_add(rv),
                BinOp::Sub => lv.wrapping_sub(rv),
                BinOp::Mul => lv.wrapping_mul(rv),
                BinOp::Div => lv / rv,
                BinOp::Mod => lv % rv,
                BinOp::BitAnd => lv & rv,
                BinOp::BitOr => lv | rv,
                BinOp::BitXor => lv ^ rv,
                BinOp::Shl => shl16(lv, rv),
                BinOp::Shr if lt == Scalar::I16 => sar16(lv, rv),
                BinOp::Shr => shr16(lv, rv),
                _ => unreachable!(),
            };
            Ok((lt, v))
        }
        _ => Err(err(e.pos, "not a constant expression".into())),
    }
}

fn shl16(v: u16, amt: u16) -> u16 {
    if amt >= 16 { 0 } else { v << amt }
}
fn shr16(v: u16, amt: u16) -> u16 {
    if amt >= 16 { 0 } else { v >> amt }
}
fn sar16(v: u16, amt: u16) -> u16 {
    if amt >= 16 {
        if v & 0x8000 != 0 { 0xFFFF } else { 0 }
    } else {
        ((v as i16) >> amt) as u16
    }
}

fn check_static(
    s: &StaticItem,
    consts: &HashMap<String, (Scalar, u16)>,
) -> Result<(StaticInfo, Vec<u16>), CompileError> {
    match &s.len {
        None => {
            let Init::Scalar(init) = &s.init else {
                return Err(err(s.pos, "array initializer on a non-array static".into()));
            };
            let v = const_eval(init, consts, Some(s.elem))?;
            Ok((StaticInfo { elem: s.elem, len: None }, vec![v]))
        }
        Some(len_expr) => {
            let len = const_eval(len_expr, consts, Some(Scalar::U16))?;
            if len == 0 {
                return Err(err(s.pos, "array length must be nonzero".into()));
            }
            let words = match &s.init {
                Init::Repeat(value, count) => {
                    let v = const_eval(value, consts, Some(s.elem))?;
                    let n = const_eval(count, consts, Some(Scalar::U16))?;
                    if n != len {
                        return Err(err(
                            s.pos,
                            format!("repeat count {n} does not match array length {len}"),
                        ));
                    }
                    vec![v; len as usize]
                }
                Init::List(values) => {
                    if values.len() != len as usize {
                        return Err(err(
                            s.pos,
                            format!("{} initializer(s) for an array of length {len}", values.len()),
                        ));
                    }
                    values
                        .iter()
                        .map(|v| const_eval(v, consts, Some(s.elem)))
                        .collect::<Result<_, _>>()?
                }
                Init::Scalar(_) => {
                    return Err(err(
                        s.pos,
                        "array static needs `[value; count]` or `[a, b, ...]`".into(),
                    ));
                }
            };
            Ok((StaticInfo { elem: s.elem, len: Some(len) }, words))
        }
    }
}

// --- Function bodies --------------------------------------------------------

struct FnCx<'g> {
    globals: &'g Globals,
    scopes: Vec<HashMap<String, (usize, Scalar)>>,
    next_slot: usize,
    max_call_args: usize,
    loop_depth: usize,
    ret: Option<Scalar>,
}

fn check_fn(f: &FnItem, globals: &Globals) -> Result<TFn, CompileError> {
    let mut cx = FnCx {
        globals,
        scopes: vec![HashMap::new()],
        next_slot: 0,
        max_call_args: 0,
        loop_depth: 0,
        ret: f.ret,
    };
    for (name, ty, pos) in &f.params {
        if cx.scopes[0].contains_key(name) {
            return Err(err(*pos, format!("duplicate parameter `{name}`")));
        }
        let slot = cx.next_slot;
        cx.next_slot += 1;
        cx.scopes[0].insert(name.clone(), (slot, *ty));
    }

    let body = cx.block(&f.body)?;

    if f.ret.is_some() && !block_returns(&body) {
        return Err(err(f.pos, format!("`{}` may finish without returning a value", f.name)));
    }

    let arg_stage = cx.next_slot;
    Ok(TFn {
        name: f.name.clone(),
        nparams: f.params.len(),
        nslots: arg_stage + cx.max_call_args,
        arg_stage,
        body,
    })
}

/// Conservative "all paths return": the block ends with a `return`, an
/// `if`/`else` whose branches both return, or a `loop` with no `break` of
/// its own.
fn block_returns(stmts: &[TStmt]) -> bool {
    match stmts.last() {
        Some(TStmt::Return(_)) => true,
        Some(TStmt::If { then, els, .. }) => block_returns(then) && block_returns(els),
        Some(TStmt::Loop { body }) => !has_own_break(body),
        _ => false,
    }
}

/// Does this loop body contain a `break` targeting it (not a nested loop)?
fn has_own_break(stmts: &[TStmt]) -> bool {
    stmts.iter().any(|s| match s {
        TStmt::Break => true,
        TStmt::If { then, els, .. } => has_own_break(then) || has_own_break(els),
        // a break inside a nested loop targets that loop
        TStmt::While { .. } | TStmt::Loop { .. } => false,
        _ => false,
    })
}

impl FnCx<'_> {
    fn lookup_local(&self, name: &str) -> Option<(usize, Scalar)> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn block(&mut self, stmts: &[Stmt]) -> Result<Vec<TStmt>, CompileError> {
        self.scopes.push(HashMap::new());
        let out = stmts.iter().map(|s| self.stmt(s)).collect();
        self.scopes.pop();
        out
    }

    fn stmt(&mut self, s: &Stmt) -> Result<TStmt, CompileError> {
        match s {
            Stmt::Let { name, ty, value } => {
                let value = self.value_expr(value, *ty)?;
                let ty = value.ty.unwrap();
                let slot = self.next_slot;
                self.next_slot += 1;
                self.scopes.last_mut().unwrap().insert(name.clone(), (slot, ty));
                Ok(TStmt::Store { slot, value })
            }
            Stmt::Assign { name, index, value, pos } => {
                self.assign(name, index.as_ref(), value, *pos)
            }
            Stmt::If { cond, then, els } => Ok(TStmt::If {
                cond: self.bool_expr(cond)?,
                then: self.block(then)?,
                els: self.block(els)?,
            }),
            Stmt::While { cond, body } => {
                let cond = self.bool_expr(cond)?;
                self.loop_depth += 1;
                let body = self.block(body)?;
                self.loop_depth -= 1;
                Ok(TStmt::While { cond, body })
            }
            Stmt::Loop { body } => {
                self.loop_depth += 1;
                let body = self.block(body)?;
                self.loop_depth -= 1;
                Ok(TStmt::Loop { body })
            }
            Stmt::Break(pos) => {
                if self.loop_depth == 0 {
                    return Err(err(*pos, "`break` outside of a loop".into()));
                }
                Ok(TStmt::Break)
            }
            Stmt::Continue(pos) => {
                if self.loop_depth == 0 {
                    return Err(err(*pos, "`continue` outside of a loop".into()));
                }
                Ok(TStmt::Continue)
            }
            Stmt::Return(value, pos) => match (self.ret, value) {
                (None, None) => Ok(TStmt::Return(None)),
                (None, Some(_)) => Err(err(*pos, "this function returns no value".into())),
                (Some(ty), Some(v)) => Ok(TStmt::Return(Some(self.value_expr(v, Some(ty))?))),
                (Some(ty), None) => Err(err(*pos, format!("expected `return` with a `{ty}` value"))),
            },
            Stmt::Expr(e) => Ok(TStmt::Expr(self.expr(e, None)?)),
        }
    }

    fn assign(
        &mut self,
        name: &str,
        index: Option<&Expr>,
        value: &Expr,
        pos: Pos,
    ) -> Result<TStmt, CompileError> {
        if let Some((slot, ty)) = self.lookup_local(name) {
            if index.is_some() {
                return Err(err(pos, format!("`{name}` is not an array")));
            }
            let value = self.value_expr(value, Some(ty))?;
            return Ok(TStmt::Store { slot, value });
        }
        if self.globals.consts.contains_key(name) {
            return Err(err(pos, format!("cannot assign to constant `{name}`")));
        }
        if let Some(info) = self.globals.statics.get(name) {
            let elem = info.elem;
            return match (info.len.is_some(), index) {
                (false, None) => Ok(TStmt::StoreStatic {
                    name: name.into(),
                    value: self.value_expr(value, Some(elem))?,
                }),
                (true, Some(idx)) => Ok(TStmt::StoreIndex {
                    name: name.into(),
                    index: self.value_expr(idx, Some(Scalar::U16))?,
                    value: self.value_expr(value, Some(elem))?,
                }),
                (true, None) => Err(err(pos, format!("array `{name}` needs an index"))),
                (false, Some(_)) => Err(err(pos, format!("`{name}` is not an array"))),
            };
        }
        Err(err(pos, format!("unknown variable `{name}`")))
    }

    /// An expression that must produce a value, optionally of a required type.
    fn value_expr(&mut self, e: &Expr, expected: Option<Scalar>) -> Result<TExpr, CompileError> {
        let t = self.expr(e, expected)?;
        let Some(ty) = t.ty else {
            return Err(err(e.pos, "this call returns no value".into()));
        };
        if let Some(want) = expected {
            if ty != want {
                return Err(err(e.pos, format!("expected `{want}`, found `{ty}`")));
            }
        }
        Ok(t)
    }

    fn bool_expr(&mut self, e: &Expr) -> Result<TExpr, CompileError> {
        self.value_expr(e, Some(Scalar::Bool))
    }

    fn expr(&mut self, e: &Expr, expected: Option<Scalar>) -> Result<TExpr, CompileError> {
        match &e.kind {
            ExprKind::Int(v) => {
                let (ty, bits) = literal(e.pos, *v, expected)?;
                Ok(TExpr { ty: Some(ty), kind: TExprKind::Int(bits) })
            }
            ExprKind::Bool(b) => {
                Ok(TExpr { ty: Some(Scalar::Bool), kind: TExprKind::Int(*b as u16) })
            }
            ExprKind::Name(name) => {
                if let Some((slot, ty)) = self.lookup_local(name) {
                    return Ok(TExpr { ty: Some(ty), kind: TExprKind::Local(slot) });
                }
                if let Some(&(ty, v)) = self.globals.consts.get(name) {
                    return Ok(TExpr { ty: Some(ty), kind: TExprKind::Int(v) });
                }
                if let Some(info) = self.globals.statics.get(name) {
                    if info.len.is_some() {
                        return Err(err(e.pos, format!("array `{name}` needs an index")));
                    }
                    return Ok(TExpr { ty: Some(info.elem), kind: TExprKind::Static(name.clone()) });
                }
                if self.globals.fns.contains_key(name) {
                    return Err(err(e.pos, format!("function `{name}` must be called")));
                }
                Err(err(e.pos, format!("unknown name `{name}`")))
            }
            ExprKind::Index(name, idx) => {
                let Some(info) = self.globals.statics.get(name) else {
                    if self.lookup_local(name).is_some() {
                        return Err(err(e.pos, format!("`{name}` is not an array")));
                    }
                    return Err(err(e.pos, format!("unknown array `{name}`")));
                };
                if info.len.is_none() {
                    return Err(err(e.pos, format!("`{name}` is not an array")));
                }
                let elem = info.elem;
                let idx = self.value_expr(idx, Some(Scalar::U16))?;
                Ok(TExpr { ty: Some(elem), kind: TExprKind::Index(name.clone(), Box::new(idx)) })
            }
            ExprKind::Call(name, args) => self.call(name, args, e.pos),
            ExprKind::Unary(UnOp::Neg, inner) => {
                if let ExprKind::Int(v) = inner.kind {
                    let (ty, bits) = literal(e.pos, -v, Some(Scalar::I16))?;
                    return Ok(TExpr { ty: Some(ty), kind: TExprKind::Int(bits) });
                }
                let inner = self.value_expr(inner, Some(Scalar::I16))?;
                Ok(TExpr {
                    ty: Some(Scalar::I16),
                    kind: TExprKind::Unary(UnOp::Neg, Box::new(inner)),
                })
            }
            ExprKind::Unary(UnOp::Not, inner) => {
                let inner = self.value_expr(inner, expected)?;
                let ty = inner.ty.unwrap();
                Ok(TExpr { ty: Some(ty), kind: TExprKind::Unary(UnOp::Not, Box::new(inner)) })
            }
            ExprKind::Cast(inner, to) => {
                let inner = self.value_expr(inner, None)?;
                if inner.ty == Some(Scalar::Bool) {
                    return Err(err(e.pos, "cannot cast `bool`".into()));
                }
                Ok(TExpr { ty: Some(*to), kind: inner.kind })
            }
            ExprKind::Binary(op, lhs, rhs) => self.binary(*op, lhs, rhs, e.pos, expected),
        }
    }

    fn binary(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        pos: Pos,
        expected: Option<Scalar>,
    ) -> Result<TExpr, CompileError> {
        let is_lit = |e: &Expr| {
            matches!(e.kind, ExprKind::Int(_))
                || matches!(&e.kind, ExprKind::Unary(UnOp::Neg, i) if matches!(i.kind, ExprKind::Int(_)))
        };

        match op {
            BinOp::LogAnd | BinOp::LogOr => {
                let l = self.bool_expr(lhs)?;
                let r = self.bool_expr(rhs)?;
                Ok(TExpr {
                    ty: Some(Scalar::Bool),
                    kind: TExprKind::Binary {
                        op,
                        operands: Scalar::Bool,
                        lhs: Box::new(l),
                        rhs: Box::new(r),
                    },
                })
            }
            BinOp::Shl | BinOp::Shr => {
                let want = expected.filter(|t| *t != Scalar::Bool);
                let l = self.value_expr(lhs, want)?;
                let lty = l.ty.unwrap();
                if lty == Scalar::Bool {
                    return Err(err(pos, "cannot shift a `bool`".into()));
                }
                let r = self.value_expr(rhs, Some(Scalar::U16))?;
                Ok(TExpr {
                    ty: Some(lty),
                    kind: TExprKind::Binary { op, operands: lty, lhs: Box::new(l), rhs: Box::new(r) },
                })
            }
            _ => {
                // Comparison or arithmetic: operands share one type. Check
                // the non-literal side first so literals adapt to it.
                let (l, r) = if is_lit(lhs) && !is_lit(rhs) {
                    let r = self.value_expr(rhs, None)?;
                    let l = self.value_expr(lhs, r.ty)?;
                    (l, r)
                } else {
                    let want = if op.is_compare() {
                        None
                    } else {
                        expected.filter(|t| *t != Scalar::Bool)
                    };
                    let l = self.value_expr(lhs, want)?;
                    let r = self.value_expr(rhs, l.ty)?;
                    (l, r)
                };
                let (lt, rt) = (l.ty.unwrap(), r.ty.unwrap());
                if lt != rt {
                    return Err(err(pos, format!("mismatched types: `{lt}` and `{rt}` (use `as`)")));
                }
                if op.is_compare() {
                    if lt == Scalar::Bool && !matches!(op, BinOp::Eq | BinOp::Ne) {
                        return Err(err(pos, "`bool` supports only `==` and `!=`".into()));
                    }
                    return Ok(TExpr {
                        ty: Some(Scalar::Bool),
                        kind: TExprKind::Binary {
                            op,
                            operands: lt,
                            lhs: Box::new(l),
                            rhs: Box::new(r),
                        },
                    });
                }
                if lt == Scalar::Bool {
                    return Err(err(pos, "integer operator on `bool`".into()));
                }
                if matches!(op, BinOp::Div | BinOp::Mod) && lt != Scalar::U16 {
                    return Err(err(pos, "`/` and `%` are `u16`-only in v1 (D018)".into()));
                }
                Ok(TExpr {
                    ty: Some(lt),
                    kind: TExprKind::Binary { op, operands: lt, lhs: Box::new(l), rhs: Box::new(r) },
                })
            }
        }
    }

    fn call(&mut self, name: &str, args: &[Expr], pos: Pos) -> Result<TExpr, CompileError> {
        if let Some((_, builtin)) = BUILTINS.iter().find(|(b, _)| *b == name) {
            let (params, ret): (&[Scalar], Option<Scalar>) = match builtin {
                Builtin::Peek | Builtin::Peek8 => (&[Scalar::U16], Some(Scalar::U16)),
                Builtin::Poke | Builtin::Poke8 => (&[Scalar::U16, Scalar::U16], None),
                Builtin::Halt | Builtin::Brk => (&[], None),
            };
            let targs = self.check_args(name, args, params, pos)?;
            return Ok(TExpr { ty: ret, kind: TExprKind::Builtin(*builtin, targs) });
        }
        let Some((params, ret)) = self.globals.fns.get(name).cloned() else {
            return Err(err(pos, format!("unknown function `{name}`")));
        };
        let targs = self.check_args(name, args, &params, pos)?;
        self.max_call_args = self.max_call_args.max(args.len());
        Ok(TExpr { ty: ret, kind: TExprKind::Call(name.to_string(), targs) })
    }

    fn check_args(
        &mut self,
        name: &str,
        args: &[Expr],
        params: &[Scalar],
        pos: Pos,
    ) -> Result<Vec<TExpr>, CompileError> {
        if args.len() != params.len() {
            return Err(err(
                pos,
                format!("`{name}` expects {} argument(s), got {}", params.len(), args.len()),
            ));
        }
        args.iter()
            .zip(params)
            .map(|(a, ty)| self.value_expr(a, Some(*ty)))
            .collect()
    }
}
