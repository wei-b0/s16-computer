//! One test per error class in specs/COMPILER.md's diagnostics table.
//! Wording is not contractual; rejection and a sensible position are.

fn fails(source: &str) -> s16c::CompileError {
    s16c::compile(source).expect_err(source)
}

#[test]
fn error_classes_are_rejected() {
    // (source, line where the error is, message fragment)
    let cases: &[(&str, usize, &str)] = &[
        // syntax
        ("fn main() { var x = ; }", 1, "expected an expression"),
        ("fn main() { if true return; }", 1, "expected `{`"),
        ("fn main() {", 1, "unclosed block"),
        // string literals (D024): the token only parses in call-argument
        // position, so it's a syntax error everywhere else
        ("fn main() { var x: u16 = \"hi\"; }", 1, "expected an expression"),
        ("fn main() { var b: bool = \"a\" == \"b\"; }", 1, "expected an expression"),
        ("static A: [u16; 1] = [1];\nfn main() { var y: u16 = A[\"hi\"]; }", 2, "expected an expression"),
        // name
        ("fn main() { x = 1; }", 1, "unknown variable"),
        ("fn main() { var a: u16 = y; }", 1, "unknown name"),
        ("fn main() { f(); }", 1, "unknown function"),
        ("fn main() { }\nfn main() { }", 2, "already defined"),
        // type
        ("fn main() { var a: u16 = 1; var b: i16 = 2; var c: u16 = a + b; }", 1, "found `i16`"),
        ("fn main() { if 1 { } }", 1, "expected `bool`"),
        ("fn f(a: u16) { }\nfn main() { f(true); }", 2, "expected `u16`"),
        ("fn f(a: u16) { }\nfn main() { f(1, 2); }", 2, "expects 1 argument"),
        // a string literal parses fine as *some* call argument (the parser
        // doesn't know the callee's signature yet) but is rejected once
        // check.rs sees it isn't going to a `str` parameter (D024)
        ("fn f(a: u16) { }\nfn main() { f(\"hi\"); }", 2, "string literal"),
        ("const K: u16 = 1;\nfn main() { K = 2; }", 2, "cannot assign to constant"),
        ("fn main() { return 1; }", 1, "returns no value"),
        ("fn f() -> u16 { return; }\nfn main() { }", 1, "expected `return` with"),
        // signedness (D018)
        ("fn main() { var a: i16 = -6; var b: i16 = a / 2; }", 1, "`u16`-only"),
        ("fn main() { var a: u16 = 5; var b: i16 = -a; }", 1, "expected `i16`"),
        // const
        ("fn g() -> u16 { return 1; }\nstatic X: u16 = g();\nfn main() { }", 2, "not a constant"),
        ("static A: [u16; 0] = [0; 0];\nfn main() { }", 1, "nonzero"),
        ("fn main() { var x: u16 = 70000; }", 1, "out of range"),
        // structure
        ("fn not_main() { }", 1, "missing `fn main()`"),
        ("fn main(a: u16) { }", 1, "must take no parameters"),
        ("fn main() { break; }", 1, "outside of a loop"),
        ("fn f() -> u16 { if true { return 1; } }\nfn main() { }", 1, "without returning"),
        // arrays
        ("static A: [u16; 2] = [1, 2];\nfn main() { var x: u16 = A; }", 2, "needs an index"),
        ("static A: [u16; 2] = [1, 2, 3];\nfn main() { }", 1, "3 initializer(s)"),
        ("fn main() { var x: u16 = 1; var y: u16 = x[0]; }", 1, "not an array"),
    ];

    for (src, line, needle) in cases {
        let e = fails(src);
        assert_eq!(e.line, *line, "line for {src:?} (got: {e})");
        assert!(e.msg.contains(needle), "{src:?}: got `{}`", e.msg);
    }
}

#[test]
fn positions_include_columns() {
    let e = fails("fn main() { var a: u16 = true; }");
    assert_eq!(e.line, 1);
    assert!(e.col > 20, "column should point into the initializer, got {}", e.col);
}
