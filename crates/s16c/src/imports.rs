//! Resolves `use` items: validates library names, then pulls in only the
//! items transitively referenced from the union of used libraries — same
//! reachability + shadowing rule as before (D020), just re-scoped by
//! explicit import instead of always scanning one merged prelude (D022).

use crate::ast::{BinOp, Expr, ExprKind, FnItem, Init, Item, Stmt, UnOp};
use crate::{CompileError, Pos};
use std::collections::{BTreeSet, HashMap, HashSet};

/// The fixed, compiler-shipped set of standard libraries (LANGUAGE.md §7).
/// Not a package manager: no external files, no user-authored libraries.
const LIBRARIES: &[(&str, &str)] = &[
    ("screen", include_str!("libs/screen.s16c")),
    ("input", include_str!("libs/input.s16c")),
    ("time", include_str!("libs/time.s16c")),
    ("random", include_str!("libs/random.s16c")),
];

pub fn resolve(items: Vec<Item>) -> Result<Vec<Item>, CompileError> {
    let mut uses: Vec<(String, Pos)> = Vec::new();
    let mut user_items = Vec::new();
    for item in items {
        match item {
            Item::Use(u) => uses.push((u.name, u.pos)),
            other => user_items.push(other),
        }
    }

    let mut requested: HashSet<&str> = HashSet::new();
    for (name, pos) in &uses {
        if !LIBRARIES.iter().any(|(lib, _)| lib == name) {
            let mut available: Vec<&str> = LIBRARIES.iter().map(|(n, _)| *n).collect();
            available.sort_unstable();
            return Err(CompileError::at(
                *pos,
                format!("no such library `{name}`; available: {}", available.join(", ")),
            ));
        }
        requested.insert(name.as_str());
    }

    // Parse every library (libraries are tiny and never depend on each
    // other, so this is cheap and needs no recursive resolution) into one
    // flat, registry-ordered list, so the merged output is deterministic
    // regardless of `use` order or BFS visit order.
    let mut global_items: Vec<(&str, Item)> = Vec::new();
    for &(lib, src) in LIBRARIES {
        let tokens = crate::lexer::lex(src)
            .unwrap_or_else(|e| panic!("s16c library `{lib}` failed to lex: {e}"));
        let items = crate::parser::parse(&tokens)
            .unwrap_or_else(|e| panic!("s16c library `{lib}` failed to parse: {e}"));
        for item in items {
            global_items.push((lib, item));
        }
    }
    let mut name_to_idx: HashMap<&str, usize> = HashMap::new();
    for (i, (lib, item)) in global_items.iter().enumerate() {
        let name = item_name(item);
        if let Some(&prev) = name_to_idx.get(name) {
            panic!(
                "s16c library bug: `{name}` is defined in both `{}` and `{lib}`",
                global_items[prev].0
            );
        }
        name_to_idx.insert(name, i);
    }
    let available: HashSet<usize> = (0..global_items.len())
        .filter(|&i| requested.contains(global_items[i].0))
        .collect();

    let user_names: HashSet<&str> = user_items.iter().map(item_name).collect();

    let mut referenced = HashSet::new();
    let mut called: Vec<(String, Pos)> = Vec::new();
    for item in &user_items {
        collect_item(item, &mut referenced, &mut called);
    }

    // BFS over referenced names, pulling in library items transitively.
    // `included` is a BTreeSet of global indices so the merged order always
    // matches registry order (screen, input, time, random; source order
    // within each), regardless of visit order — deterministic output.
    let mut included: BTreeSet<usize> = BTreeSet::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut worklist: Vec<String> = referenced.iter().cloned().collect();
    while let Some(name) = worklist.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if user_names.contains(name.as_str()) {
            continue; // shadowed: the user's own item satisfies this reference
        }
        if let Some(&idx) = name_to_idx.get(name.as_str()) {
            if available.contains(&idx) && included.insert(idx) {
                let mut more = HashSet::new();
                let mut more_called = Vec::new(); // library internals never need the hint
                collect_item(&global_items[idx].1, &mut more, &mut more_called);
                worklist.extend(more);
            }
        }
    }

    // A name called (not just referenced — see below) but left unresolved:
    // if it's a real export of a library the program didn't `use`, say so
    // instead of falling through to a bare "unknown function" error. Gated
    // to call position specifically because a bare name (`Name`/`Index`/
    // assignment target) could just as easily be an unrelated local
    // variable that happens to share a library function's name — calling
    // a name, `f(...)`, is what actually implies "this must be a function".
    // `called` is in source-appearance order, so this reports the earliest
    // offending call site, matching every other diagnostic's positioning.
    for (name, pos) in &called {
        if user_names.contains(name.as_str()) {
            continue;
        }
        if let Some(&idx) = name_to_idx.get(name.as_str()) {
            if !available.contains(&idx) {
                let lib = global_items[idx].0;
                return Err(CompileError::at(
                    *pos,
                    format!("unknown function `{name}` (found in library `{lib}` — add `use {lib};`)"),
                ));
            }
        }
    }

    let mut global_items: Vec<Option<Item>> = global_items.into_iter().map(|(_, i)| Some(i)).collect();
    let mut result = user_items;
    for idx in included {
        result.push(global_items[idx].take().unwrap());
    }
    Ok(result)
}

fn item_name(item: &Item) -> &str {
    match item {
        Item::Fn(f) => &f.name,
        Item::Static(s) => &s.name,
        Item::Const(c) => &c.name,
        Item::Use(u) => &u.name,
    }
}

fn collect_item(item: &Item, out: &mut HashSet<String>, called: &mut Vec<(String, Pos)>) {
    match item {
        Item::Fn(FnItem { body, .. }) => {
            for s in body {
                collect_stmt(s, out, called);
            }
        }
        Item::Static(s) => {
            if let Some(len) = &s.len {
                collect_expr(len, out, called);
            }
            match &s.init {
                Init::Scalar(e) => collect_expr(e, out, called),
                Init::Repeat(v, n) => {
                    collect_expr(v, out, called);
                    collect_expr(n, out, called);
                }
                Init::List(vs) => vs.iter().for_each(|e| collect_expr(e, out, called)),
            }
        }
        Item::Const(c) => collect_expr(&c.value, out, called),
        Item::Use(_) => {}
    }
}

fn collect_stmt(s: &Stmt, out: &mut HashSet<String>, called: &mut Vec<(String, Pos)>) {
    match s {
        Stmt::Let { value, .. } => collect_expr(value, out, called),
        Stmt::Assign { name, index, value, .. } => {
            out.insert(name.clone());
            if let Some(idx) = index {
                collect_expr(idx, out, called);
            }
            collect_expr(value, out, called);
        }
        Stmt::If { cond, then, els } => {
            collect_expr(cond, out, called);
            then.iter().for_each(|s| collect_stmt(s, out, called));
            els.iter().for_each(|s| collect_stmt(s, out, called));
        }
        Stmt::While { cond, body } => {
            collect_expr(cond, out, called);
            body.iter().for_each(|s| collect_stmt(s, out, called));
        }
        Stmt::Loop { body } => body.iter().for_each(|s| collect_stmt(s, out, called)),
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Return(value, _) => {
            if let Some(e) = value {
                collect_expr(e, out, called);
            }
        }
        Stmt::Expr(e) => collect_expr(e, out, called),
    }
}

fn collect_expr(e: &Expr, out: &mut HashSet<String>, called: &mut Vec<(String, Pos)>) {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Bool(_) => {}
        ExprKind::Name(name) => {
            out.insert(name.clone());
        }
        ExprKind::Index(name, idx) => {
            out.insert(name.clone());
            collect_expr(idx, out, called);
        }
        ExprKind::Call(name, args) => {
            out.insert(name.clone());
            called.push((name.clone(), e.pos));
            args.iter().for_each(|a| collect_expr(a, out, called));
        }
        ExprKind::Unary(UnOp::Neg | UnOp::Not, inner) => collect_expr(inner, out, called),
        ExprKind::Binary(BinOp::Add, l, r)
        | ExprKind::Binary(BinOp::Sub, l, r)
        | ExprKind::Binary(BinOp::Mul, l, r)
        | ExprKind::Binary(BinOp::Div, l, r)
        | ExprKind::Binary(BinOp::Mod, l, r)
        | ExprKind::Binary(BinOp::BitAnd, l, r)
        | ExprKind::Binary(BinOp::BitOr, l, r)
        | ExprKind::Binary(BinOp::BitXor, l, r)
        | ExprKind::Binary(BinOp::Shl, l, r)
        | ExprKind::Binary(BinOp::Shr, l, r)
        | ExprKind::Binary(BinOp::Eq, l, r)
        | ExprKind::Binary(BinOp::Ne, l, r)
        | ExprKind::Binary(BinOp::Lt, l, r)
        | ExprKind::Binary(BinOp::Le, l, r)
        | ExprKind::Binary(BinOp::Gt, l, r)
        | ExprKind::Binary(BinOp::Ge, l, r)
        | ExprKind::Binary(BinOp::LogAnd, l, r)
        | ExprKind::Binary(BinOp::LogOr, l, r) => {
            collect_expr(l, out, called);
            collect_expr(r, out, called);
        }
        ExprKind::Cast(inner, _) => collect_expr(inner, out, called),
    }
}
