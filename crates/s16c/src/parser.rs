//! Recursive-descent parser following the EBNF in LANGUAGE.md §8, with
//! Rust's operator-precedence ladder.

use crate::ast::*;
use crate::lexer::{Tok, Token};
use crate::{CompileError, Pos};

pub fn parse(tokens: &[Token]) -> Result<Vec<Item>, CompileError> {
    let mut p = Parser { tokens, i: 0 };
    let mut items = Vec::new();
    while p.peek().is_some() {
        items.push(p.item()?);
    }
    Ok(items)
}

struct Parser<'a> {
    tokens: &'a [Token],
    i: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.i).map(|t| &t.tok)
    }

    fn pos(&self) -> Pos {
        self.tokens
            .get(self.i)
            .or_else(|| self.tokens.last())
            .map(|t| t.pos)
            .unwrap_or(Pos { line: 1, col: 1 })
    }

    fn next(&mut self) -> Option<&Tok> {
        let t = self.tokens.get(self.i).map(|t| &t.tok);
        self.i += 1;
        t
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == Some(tok) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn err(&self, msg: String) -> CompileError {
        CompileError::at(self.pos(), msg)
    }

    fn expect(&mut self, tok: Tok, what: &str) -> Result<(), CompileError> {
        if self.eat(&tok) {
            Ok(())
        } else {
            Err(self.err(format!("expected {what}")))
        }
    }

    fn ident(&mut self, what: &str) -> Result<(String, Pos), CompileError> {
        let pos = self.pos();
        match self.next() {
            Some(Tok::Ident(name)) => Ok((name.clone(), pos)),
            _ => Err(CompileError::at(pos, format!("expected {what}"))),
        }
    }

    fn scalar(&mut self) -> Result<Scalar, CompileError> {
        match self.next() {
            Some(Tok::U16) => Ok(Scalar::U16),
            Some(Tok::I16) => Ok(Scalar::I16),
            Some(Tok::Bool) => Ok(Scalar::Bool),
            _ => {
                self.i -= 1;
                Err(self.err("expected a type (`u16`, `i16`, or `bool`)".into()))
            }
        }
    }

    /// A parameter's type: `str` (D024, parameter-position-only) or an
    /// ordinary scalar. Never valid for `var`/`static`/`const`/return types.
    fn param_kind(&mut self) -> Result<ParamKind, CompileError> {
        if self.eat(&Tok::StrKw) {
            Ok(ParamKind::Str)
        } else {
            self.scalar().map(ParamKind::Value)
        }
    }

    // --- Items ---------------------------------------------------------------

    fn item(&mut self) -> Result<Item, CompileError> {
        match self.peek() {
            Some(Tok::Fn) => self.function().map(Item::Fn),
            Some(Tok::Static) => self.static_item().map(Item::Static),
            Some(Tok::Const) => self.const_item().map(Item::Const),
            Some(Tok::Use) => self.use_item().map(Item::Use),
            _ => Err(self.err("expected `fn`, `static`, `const`, or `use`".into())),
        }
    }

    fn use_item(&mut self) -> Result<UseItem, CompileError> {
        self.expect(Tok::Use, "`use`")?;
        let (name, pos) = self.ident("library name")?;
        self.expect(Tok::Semi, "`;` after `use`")?;
        Ok(UseItem { name, pos })
    }

    fn function(&mut self) -> Result<FnItem, CompileError> {
        self.expect(Tok::Fn, "`fn`")?;
        let (name, pos) = self.ident("function name")?;
        self.expect(Tok::LParen, "`(` after function name")?;
        let mut params = Vec::new();
        if !self.eat(&Tok::RParen) {
            loop {
                let (pname, ppos) = self.ident("parameter name")?;
                self.expect(Tok::Colon, "`:` after parameter name")?;
                params.push((pname, self.param_kind()?, ppos));
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(Tok::RParen, "`)` after parameters")?;
        }
        let ret = if self.eat(&Tok::Arrow) {
            Some(self.scalar()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(FnItem { name, pos, params, ret, body })
    }

    fn static_item(&mut self) -> Result<StaticItem, CompileError> {
        self.expect(Tok::Static, "`static`")?;
        let (name, pos) = self.ident("static name")?;
        self.expect(Tok::Colon, "`:` after static name")?;

        let (elem, len) = if self.eat(&Tok::LBracket) {
            let elem = self.scalar()?;
            if elem == Scalar::Bool {
                return Err(CompileError::at(pos, "array element type must be `u16` or `i16`".into()));
            }
            self.expect(Tok::Semi, "`;` in array type `[T; N]`")?;
            let len = self.expr()?;
            self.expect(Tok::RBracket, "`]` after array length")?;
            (elem, Some(len))
        } else {
            (self.scalar()?, None)
        };

        self.expect(Tok::Assign, "`=` in static initializer")?;
        let init = if self.eat(&Tok::LBracket) {
            let first = self.expr()?;
            if self.eat(&Tok::Semi) {
                let count = self.expr()?;
                self.expect(Tok::RBracket, "`]` after `[value; count]`")?;
                Init::Repeat(first, count)
            } else {
                let mut values = vec![first];
                while self.eat(&Tok::Comma) {
                    values.push(self.expr()?);
                }
                self.expect(Tok::RBracket, "`]` after array values")?;
                Init::List(values)
            }
        } else {
            Init::Scalar(self.expr()?)
        };
        self.expect(Tok::Semi, "`;` after static item")?;
        Ok(StaticItem { name, pos, elem, len, init })
    }

    fn const_item(&mut self) -> Result<ConstItem, CompileError> {
        self.expect(Tok::Const, "`const`")?;
        let (name, pos) = self.ident("const name")?;
        self.expect(Tok::Colon, "`:` after const name")?;
        let ty = self.scalar()?;
        self.expect(Tok::Assign, "`=` in const definition")?;
        let value = self.expr()?;
        self.expect(Tok::Semi, "`;` after const item")?;
        Ok(ConstItem { name, pos, ty, value })
    }

    // --- Statements ----------------------------------------------------------

    fn block(&mut self) -> Result<Vec<Stmt>, CompileError> {
        self.expect(Tok::LBrace, "`{`")?;
        let mut stmts = Vec::new();
        while !self.eat(&Tok::RBrace) {
            if self.peek().is_none() {
                return Err(self.err("unclosed block: expected `}`".into()));
            }
            stmts.push(self.stmt()?);
        }
        Ok(stmts)
    }

    fn stmt(&mut self) -> Result<Stmt, CompileError> {
        let pos = self.pos();
        match self.peek() {
            Some(Tok::Var) => {
                self.next();
                let (name, _) = self.ident("variable name")?;
                let ty = if self.eat(&Tok::Colon) { Some(self.scalar()?) } else { None };
                self.expect(Tok::Assign, "`=` in `var` (variables must be initialized)")?;
                let value = self.expr()?;
                self.expect(Tok::Semi, "`;` after `var`")?;
                Ok(Stmt::Let { name, ty, value })
            }
            Some(Tok::If) => self.if_stmt(),
            Some(Tok::While) => {
                self.next();
                let cond = self.expr()?;
                let body = self.block()?;
                Ok(Stmt::While { cond, body })
            }
            Some(Tok::Loop) => {
                self.next();
                let body = self.block()?;
                Ok(Stmt::Loop { body })
            }
            Some(Tok::Break) => {
                self.next();
                self.expect(Tok::Semi, "`;` after `break`")?;
                Ok(Stmt::Break(pos))
            }
            Some(Tok::Continue) => {
                self.next();
                self.expect(Tok::Semi, "`;` after `continue`")?;
                Ok(Stmt::Continue(pos))
            }
            Some(Tok::Return) => {
                self.next();
                let value = if self.peek() == Some(&Tok::Semi) {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.expect(Tok::Semi, "`;` after `return`")?;
                Ok(Stmt::Return(value, pos))
            }
            _ => {
                // Expression or assignment: parse an expression, then see if
                // an `=` follows and convert the expression to a place.
                let expr = self.expr()?;
                if self.eat(&Tok::Assign) {
                    let (name, index) = match expr.kind {
                        ExprKind::Name(name) => (name, None),
                        ExprKind::Index(name, idx) => (name, Some(*idx)),
                        _ => {
                            return Err(CompileError::at(
                                expr.pos,
                                "invalid assignment target (expected a variable or `array[index]`)".into(),
                            ))
                        }
                    };
                    let value = self.expr()?;
                    self.expect(Tok::Semi, "`;` after assignment")?;
                    Ok(Stmt::Assign { name, index, value, pos })
                } else {
                    self.expect(Tok::Semi, "`;` after expression")?;
                    Ok(Stmt::Expr(expr))
                }
            }
        }
    }

    fn if_stmt(&mut self) -> Result<Stmt, CompileError> {
        self.expect(Tok::If, "`if`")?;
        let cond = self.expr()?;
        let then = self.block()?;
        let els = if self.eat(&Tok::Else) {
            if self.peek() == Some(&Tok::If) {
                vec![self.if_stmt()?]
            } else {
                self.block()?
            }
        } else {
            Vec::new()
        };
        Ok(Stmt::If { cond, then, els })
    }

    // --- Expressions (precedence ladder, LANGUAGE.md §5) ---------------------

    fn expr(&mut self) -> Result<Expr, CompileError> {
        self.or_expr()
    }

    /// A single call argument: a string literal (D024, legal *only* here —
    /// no other call to `expr()` in this file ever sees `Tok::Str`, so a
    /// string literal anywhere else is a parse error, not a type error), or
    /// an ordinary expression.
    fn call_arg(&mut self) -> Result<Expr, CompileError> {
        let pos = self.pos();
        if let Some(Tok::Str(_)) = self.peek() {
            let Some(Tok::Str(bytes)) = self.next().cloned() else { unreachable!() };
            Ok(Expr { kind: ExprKind::Str(bytes), pos })
        } else {
            self.expr()
        }
    }

    fn binary_level(
        &mut self,
        next: fn(&mut Self) -> Result<Expr, CompileError>,
        ops: &[(Tok, BinOp)],
    ) -> Result<Expr, CompileError> {
        let mut lhs = next(self)?;
        'outer: loop {
            for (tok, op) in ops {
                if self.peek() == Some(tok) {
                    let pos = self.pos();
                    self.next();
                    let rhs = next(self)?;
                    lhs = Expr {
                        kind: ExprKind::Binary(*op, Box::new(lhs), Box::new(rhs)),
                        pos,
                    };
                    continue 'outer;
                }
            }
            return Ok(lhs);
        }
    }

    fn or_expr(&mut self) -> Result<Expr, CompileError> {
        self.binary_level(Self::and_expr, &[(Tok::OrOr, BinOp::LogOr)])
    }

    fn and_expr(&mut self) -> Result<Expr, CompileError> {
        self.binary_level(Self::cmp_expr, &[(Tok::AndAnd, BinOp::LogAnd)])
    }

    /// Non-associative: at most one comparison (`a < b < c` is a parse error).
    fn cmp_expr(&mut self) -> Result<Expr, CompileError> {
        let lhs = self.bitor_expr()?;
        let op = match self.peek() {
            Some(Tok::Eq) => BinOp::Eq,
            Some(Tok::Ne) => BinOp::Ne,
            Some(Tok::Lt) => BinOp::Lt,
            Some(Tok::Le) => BinOp::Le,
            Some(Tok::Gt) => BinOp::Gt,
            Some(Tok::Ge) => BinOp::Ge,
            _ => return Ok(lhs),
        };
        let pos = self.pos();
        self.next();
        let rhs = self.bitor_expr()?;
        if matches!(
            self.peek(),
            Some(Tok::Eq | Tok::Ne | Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge)
        ) {
            return Err(self.err("comparisons are non-associative; parenthesize".into()));
        }
        Ok(Expr {
            kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
            pos,
        })
    }

    fn bitor_expr(&mut self) -> Result<Expr, CompileError> {
        self.binary_level(Self::bitxor_expr, &[(Tok::Pipe, BinOp::BitOr)])
    }

    fn bitxor_expr(&mut self) -> Result<Expr, CompileError> {
        self.binary_level(Self::bitand_expr, &[(Tok::Caret, BinOp::BitXor)])
    }

    fn bitand_expr(&mut self) -> Result<Expr, CompileError> {
        self.binary_level(Self::shift_expr, &[(Tok::Amp, BinOp::BitAnd)])
    }

    fn shift_expr(&mut self) -> Result<Expr, CompileError> {
        self.binary_level(Self::add_expr, &[(Tok::Shl, BinOp::Shl), (Tok::Shr, BinOp::Shr)])
    }

    fn add_expr(&mut self) -> Result<Expr, CompileError> {
        self.binary_level(Self::mul_expr, &[(Tok::Plus, BinOp::Add), (Tok::Minus, BinOp::Sub)])
    }

    fn mul_expr(&mut self) -> Result<Expr, CompileError> {
        self.binary_level(
            Self::cast_expr,
            &[(Tok::Star, BinOp::Mul), (Tok::Slash, BinOp::Div), (Tok::Percent, BinOp::Mod)],
        )
    }

    fn cast_expr(&mut self) -> Result<Expr, CompileError> {
        let mut e = self.unary_expr()?;
        while self.peek() == Some(&Tok::As) {
            let pos = self.pos();
            self.next();
            let ty = match self.next() {
                Some(Tok::U16) => Scalar::U16,
                Some(Tok::I16) => Scalar::I16,
                _ => {
                    self.i -= 1;
                    return Err(self.err("`as` casts only to `u16` or `i16`".into()));
                }
            };
            e = Expr { kind: ExprKind::Cast(Box::new(e), ty), pos };
        }
        Ok(e)
    }

    fn unary_expr(&mut self) -> Result<Expr, CompileError> {
        let pos = self.pos();
        if self.eat(&Tok::Minus) {
            let e = self.unary_expr()?;
            Ok(Expr { kind: ExprKind::Unary(UnOp::Neg, Box::new(e)), pos })
        } else if self.eat(&Tok::Bang) {
            let e = self.unary_expr()?;
            Ok(Expr { kind: ExprKind::Unary(UnOp::Not, Box::new(e)), pos })
        } else {
            self.postfix_expr()
        }
    }

    fn postfix_expr(&mut self) -> Result<Expr, CompileError> {
        let pos = self.pos();
        match self.next().cloned() {
            Some(Tok::Int(v)) => Ok(Expr { kind: ExprKind::Int(v), pos }),
            Some(Tok::True) => Ok(Expr { kind: ExprKind::Bool(true), pos }),
            Some(Tok::False) => Ok(Expr { kind: ExprKind::Bool(false), pos }),
            Some(Tok::LParen) => {
                let e = self.expr()?;
                self.expect(Tok::RParen, "`)`")?;
                Ok(e)
            }
            Some(Tok::Ident(first)) => {
                // `module::name` is pure string-level namespacing (D023):
                // fold it into one flat `module::name` string and fall
                // through the existing call/index/bare-name branches
                // unchanged. `::` can never appear in a real identifier, so
                // this can't collide with any user-written name.
                let name = if self.eat(&Tok::PathSep) {
                    let (second, _) = self.ident("name after `::`")?;
                    format!("{first}::{second}")
                } else {
                    first
                };
                if self.eat(&Tok::LParen) {
                    let mut args = Vec::new();
                    if !self.eat(&Tok::RParen) {
                        loop {
                            args.push(self.call_arg()?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                        self.expect(Tok::RParen, "`)` after arguments")?;
                    }
                    Ok(Expr { kind: ExprKind::Call(name, args), pos })
                } else if self.eat(&Tok::LBracket) {
                    let idx = self.expr()?;
                    self.expect(Tok::RBracket, "`]` after index")?;
                    Ok(Expr { kind: ExprKind::Index(name, Box::new(idx)), pos })
                } else {
                    Ok(Expr { kind: ExprKind::Name(name), pos })
                }
            }
            _ => {
                self.i -= 1;
                Err(self.err("expected an expression".into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse_ok(src: &str) -> Vec<Item> {
        parse(&lex(src).unwrap()).unwrap()
    }

    #[test]
    fn parses_a_function() {
        let items = parse_ok("fn add(a: u16, b: u16) -> u16 { return a + b; }");
        let Item::Fn(f) = &items[0] else { panic!() };
        assert_eq!(f.name, "add");
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.ret, Some(Scalar::U16));
        assert_eq!(f.body.len(), 1);
    }

    #[test]
    fn rust_precedence_amp_binds_tighter_than_ne() {
        // keys & 4 != 0  parses as  (keys & 4) != 0
        let items = parse_ok("fn f(keys: u16) { if keys & 4 != 0 { } }");
        let Item::Fn(f) = &items[0] else { panic!() };
        let Stmt::If { cond, .. } = &f.body[0] else { panic!() };
        let ExprKind::Binary(BinOp::Ne, lhs, _) = &cond.kind else {
            panic!("expected != at top")
        };
        assert!(matches!(lhs.kind, ExprKind::Binary(BinOp::BitAnd, ..)));
    }

    #[test]
    fn comparisons_are_non_associative() {
        let e = parse(&lex("fn f() { var x: bool = 1 < 2 < 3; }").unwrap()).unwrap_err();
        assert!(e.msg.contains("non-associative"), "{}", e.msg);
    }

    #[test]
    fn statics_and_consts() {
        let items = parse_ok("const N: u16 = 5;\nstatic A: [u16; 3] = [1, 2, 3];\nstatic X: i16 = -1;");
        assert!(matches!(items[0], Item::Const(_)));
        let Item::Static(a) = &items[1] else { panic!() };
        assert!(a.len.is_some());
        assert!(matches!(a.init, Init::List(_)));
        let Item::Static(x) = &items[2] else { panic!() };
        assert!(x.len.is_none());
    }

    #[test]
    fn assignment_targets() {
        parse_ok("fn f() { x = 1; a[2] = 3; g(); }");
        let e = parse(&lex("fn f() { 1 + 2 = 3; }").unwrap()).unwrap_err();
        assert!(e.msg.contains("assignment target"), "{}", e.msg);
    }

    #[test]
    fn use_items() {
        let items = parse_ok("use screen;\nuse input;\nfn main() { }");
        let Item::Use(u) = &items[0] else { panic!() };
        assert_eq!(u.name, "screen");
        let Item::Use(u) = &items[1] else { panic!() };
        assert_eq!(u.name, "input");
    }

    #[test]
    fn module_qualified_calls_and_names() {
        let items = parse_ok("fn f() { screen::put(1, 2, 3, screen::GREEN); }");
        let Item::Fn(f) = &items[0] else { panic!() };
        let Stmt::Expr(e) = &f.body[0] else { panic!() };
        let ExprKind::Call(name, args) = &e.kind else { panic!() };
        assert_eq!(name, "screen::put");
        assert!(matches!(&args[3].kind, ExprKind::Name(n) if n == "screen::GREEN"));
    }

    #[test]
    fn str_param_kind() {
        let items = parse_ok("fn print(s: str) { }");
        let Item::Fn(f) = &items[0] else { panic!() };
        assert_eq!(f.params[0].1, ParamKind::Str);
    }

    #[test]
    fn string_literal_only_legal_as_a_call_argument() {
        parse_ok("fn f() { debug::print(\"hi\"); }");
        for bad in [
            "fn f() { var x: u16 = \"hi\"; }",
            "fn f() { var b: bool = \"a\" == \"b\"; }",
            "fn f() { var a: [u16; 1] = [1]; var y: u16 = a[\"hi\"]; }",
        ] {
            assert!(parse(&lex(bad).unwrap()).is_err(), "expected parse error for: {bad}");
        }
    }

    #[test]
    fn else_if_chains() {
        let items = parse_ok("fn f(x: u16) { if x == 0 { } else if x == 1 { } else { } }");
        let Item::Fn(f) = &items[0] else { panic!() };
        let Stmt::If { els, .. } = &f.body[0] else { panic!() };
        assert!(matches!(els[0], Stmt::If { .. }));
    }
}
