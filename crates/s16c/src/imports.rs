//! Resolves `use` items and `module::name` qualification (D022, D023):
//! validates library names, then pulls in only the items transitively
//! referenced from the union of used libraries — same reachability rule as
//! before (D020), now scoped per-library instead of one flat namespace, so
//! two libraries can share an internal helper name without colliding.

use crate::ast::{BinOp, Expr, ExprKind, FnItem, Init, Item, Stmt, UnOp};
use crate::{CompileError, Pos};
use std::collections::{BTreeSet, HashMap, HashSet};

/// The fixed, compiler-shipped set of standard libraries (LANGUAGE.md §7).
/// Not a package manager: no external files, no user-authored libraries.
const LIBRARIES: &[(&str, &str)] = &[
    ("screen", include_str!("libs/screen.s16c")),
    ("input", include_str!("libs/input.s16c")),
    ("time", include_str!("libs/time.s16c")),
    ("rng", include_str!("libs/rng.s16c")),
    ("debug", include_str!("libs/debug.s16c")),
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

    // Parse every library eagerly (needed for the "add `use X;`" hint below,
    // regardless of what's actually `use`d) into one flat, registry-ordered
    // list — included items always come out in this order (screen, input,
    // time, rng, debug; source order within each), regardless of `use`
    // order or BFS visit order.
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

    // Keyed by the exact same "lib::bare" string a qualified user call
    // already produces (D023) — a user reference and a library registration
    // resolve through literally the same lookup, no separate mangling step.
    // Two libraries sharing an internal helper name (e.g. `write_byte` in
    // both `screen` and `debug`) get distinct keys and don't collide; only
    // a genuine duplicate *within* one library is a bug.
    let mut name_to_idx: HashMap<String, usize> = HashMap::new();
    for (i, (lib, item)) in global_items.iter().enumerate() {
        let bare = item_name(item);
        let key = format!("{lib}::{bare}");
        if let Some(&prev) = name_to_idx.get(&key) {
            panic!("s16c library bug: `{bare}` is defined twice in library `{lib}` (index {prev})");
        }
        name_to_idx.insert(key, i);
    }
    let available: HashSet<usize> = (0..global_items.len())
        .filter(|&i| requested.contains(global_items[i].0))
        .collect();

    let mut referenced = HashSet::new();
    let mut called: Vec<(String, Pos)> = Vec::new();
    for item in &user_items {
        collect_item(item, &mut referenced, &mut called);
    }

    // BFS. Each worklist entry is a name plus (if it came from inside an
    // already-included library item's own body) the library that item
    // belongs to: a bare intra-library reference resolves against THAT
    // library (libraries keep calling their own siblings by bare name,
    // exactly as before D023); a `lib::name` reference always resolves
    // against its own explicit `lib`, regardless of context; a bare name
    // with no context — i.e. straight from user code — never resolves
    // against any library at all. There is no bare-name fallback after
    // `use` anymore: only `lib::name` is ever callable from user code.
    let mut included: BTreeSet<usize> = BTreeSet::new();
    let mut seen: HashSet<(Option<&str>, String)> = HashSet::new();
    let mut worklist: Vec<(Option<&str>, String)> = referenced.iter().map(|n| (None, n.clone())).collect();
    while let Some((ctx, name)) = worklist.pop() {
        if !seen.insert((ctx, name.clone())) {
            continue;
        }
        let key = match name.split_once("::") {
            Some((lib, bare)) => Some(format!("{lib}::{bare}")),
            None => ctx.map(|lib| format!("{lib}::{name}")),
        };
        let Some(key) = key else { continue };
        let Some(&idx) = name_to_idx.get(&key) else { continue };
        if !available.contains(&idx) || !included.insert(idx) {
            continue;
        }
        let lib = global_items[idx].0;
        let mut more = HashSet::new();
        let mut more_called = Vec::new(); // library internals never need the hint
        collect_item(&global_items[idx].1, &mut more, &mut more_called);
        worklist.extend(more.into_iter().map(|n| (Some(lib), n)));
    }

    // A name *called* (not just referenced — see below) but left
    // unresolved: if it's a real qualified export of a library the program
    // didn't `use`, say so instead of falling through to a bare
    // "unknown function" error. Gated to call position specifically because
    // a bare name (`Name`/`Index`/assignment target) could just as easily
    // be an unrelated local variable — calling a name, `f(...)`, is what
    // actually implies "this must be a function". `called` is in
    // source-appearance order, so this reports the earliest offending call
    // site, matching every other diagnostic's positioning. Only qualified
    // (`lib::name`) calls ever reach this — a bare call is never a library
    // reference at all anymore, so it just falls through to check.rs's
    // ordinary unknown-function error with no hint.
    for (name, pos) in &called {
        let Some((lib, bare)) = name.split_once("::") else { continue };
        let key = format!("{lib}::{bare}");
        if let Some(&idx) = name_to_idx.get(&key) {
            if !available.contains(&idx) {
                return Err(CompileError::at(
                    *pos,
                    format!("unknown function `{name}` (found in library `{lib}` — add `use {lib};`)"),
                ));
            }
        }
    }

    // Qualify: give every included item its "lib::bare" name, and rewrite
    // same-library bare self/sibling references inside its body to match —
    // otherwise a library function's own internal calls (e.g. screen's
    // `clear` calling bare `put`) would dangle once `put`'s *item* is
    // renamed to `screen::put` and check.rs can no longer find bare `put`.
    for &idx in &included {
        let lib = global_items[idx].0;
        qualify_item(&mut global_items[idx].1, lib, &name_to_idx);
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

/// If `name` is bare (no `::`) and `lib::name` is a real item in `lib`,
/// rewrite it to that qualified form in place. Leaves local-variable and
/// parameter references alone — those never match a `lib::name` key.
fn maybe_qualify(name: &mut String, lib: &str, name_to_idx: &HashMap<String, usize>) {
    if !name.contains("::") {
        let key = format!("{lib}::{name}");
        if name_to_idx.contains_key(&key) {
            *name = key;
        }
    }
}

fn qualify_item(item: &mut Item, lib: &str, name_to_idx: &HashMap<String, usize>) {
    match item {
        Item::Fn(f) => {
            f.name = format!("{lib}::{}", f.name);
            for s in &mut f.body {
                qualify_stmt(s, lib, name_to_idx);
            }
        }
        Item::Static(s) => {
            s.name = format!("{lib}::{}", s.name);
            if let Some(len) = &mut s.len {
                qualify_expr(len, lib, name_to_idx);
            }
            match &mut s.init {
                Init::Scalar(e) => qualify_expr(e, lib, name_to_idx),
                Init::Repeat(v, n) => {
                    qualify_expr(v, lib, name_to_idx);
                    qualify_expr(n, lib, name_to_idx);
                }
                Init::List(vs) => vs.iter_mut().for_each(|e| qualify_expr(e, lib, name_to_idx)),
            }
        }
        Item::Const(c) => {
            c.name = format!("{lib}::{}", c.name);
            qualify_expr(&mut c.value, lib, name_to_idx);
        }
        Item::Use(_) => {}
    }
}

fn qualify_stmt(s: &mut Stmt, lib: &str, name_to_idx: &HashMap<String, usize>) {
    match s {
        Stmt::Let { value, .. } => qualify_expr(value, lib, name_to_idx),
        Stmt::Assign { name, index, value, .. } => {
            maybe_qualify(name, lib, name_to_idx);
            if let Some(idx) = index {
                qualify_expr(idx, lib, name_to_idx);
            }
            qualify_expr(value, lib, name_to_idx);
        }
        Stmt::If { cond, then, els } => {
            qualify_expr(cond, lib, name_to_idx);
            then.iter_mut().for_each(|s| qualify_stmt(s, lib, name_to_idx));
            els.iter_mut().for_each(|s| qualify_stmt(s, lib, name_to_idx));
        }
        Stmt::While { cond, body } => {
            qualify_expr(cond, lib, name_to_idx);
            body.iter_mut().for_each(|s| qualify_stmt(s, lib, name_to_idx));
        }
        Stmt::Loop { body } => body.iter_mut().for_each(|s| qualify_stmt(s, lib, name_to_idx)),
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Return(value, _) => {
            if let Some(e) = value {
                qualify_expr(e, lib, name_to_idx);
            }
        }
        Stmt::Expr(e) => qualify_expr(e, lib, name_to_idx),
    }
}

fn qualify_expr(e: &mut Expr, lib: &str, name_to_idx: &HashMap<String, usize>) {
    match &mut e.kind {
        ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) => {}
        ExprKind::Name(name) => maybe_qualify(name, lib, name_to_idx),
        ExprKind::Index(name, idx) => {
            maybe_qualify(name, lib, name_to_idx);
            qualify_expr(idx, lib, name_to_idx);
        }
        ExprKind::Call(name, args) => {
            maybe_qualify(name, lib, name_to_idx);
            args.iter_mut().for_each(|a| qualify_expr(a, lib, name_to_idx));
        }
        ExprKind::Unary(_, inner) | ExprKind::Cast(inner, _) => qualify_expr(inner, lib, name_to_idx),
        ExprKind::Binary(_, l, r) => {
            qualify_expr(l, lib, name_to_idx);
            qualify_expr(r, lib, name_to_idx);
        }
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
        ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) => {}
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
