# s16c Compiler Specification

The contract of `s16c`, the native compiler for the language defined in
[LANGUAGE.md](LANGUAGE.md). Like [ASSEMBLER.md](ASSEMBLER.md), this pins
observable behavior — invocation, output, diagnostics, and what the emitted
code must do — not internal design. Implementation: `crates/s16c`.

## Pipeline

```
.s16c source
   │  lex        (tokens, with line:col positions)
   │  parse      (AST, grammar per LANGUAGE.md §8)
   │  imports    (resolve `use` items into referenced library items — below)
   │  typecheck  (types, signedness, const folding, name resolution)
   │  codegen    (ABI-conforming assembly)
   ▼
.s16 assembly text  ──(s16-asm)──▶  .bin image
```

The compiler emits **assembly text**, not machine code: `s16-asm` is the
backend (D013). Emitted assembly must assemble cleanly with the stock
assembler — the compiler may use any label names and layout inside the rules
below, and the text should be readable (source-line comments encouraged),
since it doubles as the debugging view of compiled programs.

## Standard Library Merge

The five standard libraries (LANGUAGE.md §7) are s16c source. A library's
items are visible to a program only if the program `use`s that library, and
are only ever callable through their full `module::name` path (D019, D022,
D023). An implementation must:

1. **Validate `use` items against the fixed library registry** (`screen`,
   `input`, `time`, `rng`, `debug`). An unknown name is an error naming the
   available libraries; a name `use`d more than once is not an error (later
   `use`s of an already-`use`d library are simply redundant).
2. **Include a library item only if it's referenced**, directly or
   transitively, from the user's source **and** its library was `use`d — a
   qualified call, a qualified name use, or an array index counts as a
   reference. A library function calling another function in the *same*
   library (by bare name, internally — see point 6) counts as that function
   referencing it, so the inclusion set is a transitive closure within the
   `use`d libraries, not just a first-level scan.
3. **There is no bare-name fallback.** Unlike before module-qualification
   (D020), a program's own `fn`/`static`/`const` can never collide with, or
   shadow, a library item — a program can't write `::` in a name it
   defines, so the two namespaces (a program's own flat names, and every
   library's `module::name` names) can't overlap in the first place. A bare
   call to a name that happens to match a library export by coincidence
   (e.g. calling unqualified `put(...)` when only `screen::put` exists) is
   a plain unknown-function error, not a hint and not a resolution to the
   library.
4. **Merge deterministically.** The same source must produce the same
   output on every build — included library items appear in a fixed order
   (library registry order, then source order within each library), not in
   whatever order a reachability scan happens to visit them.
5. **Diagnose a call to an un-`use`d library helpfully.** If the program
   calls a *qualified* name (call position specifically — see the note
   below) whose library is a real one it didn't `use`, the error must say
   so and name the library, rather than reporting a bare "unknown
   function." This is a depth requirement, not a wording one: an
   implementation may phrase it differently, but a program that calls
   `screen::put` without `use screen;` must get a more specific answer than
   a generic unknown-name error.
   - This diagnostic is scoped to **call position only**
     (`module::name(...)`) — a bare reference to an unqualified name that
     happens to coincide with some library's internal helper is **not** a
     library reference at all under module-qualification, and must **not**
     trigger it; that's a plain unknown-name error like any other.
6. **Library internals are named once, qualified, everywhere.** An
   included library item's own name is rewritten to its qualified form
   (`put` → `screen::put`) — and so is every same-library bare reference
   inside its own body, so that e.g. `screen::clear`'s internal call to
   `put` still resolves after `put` itself becomes `screen::put`. Two
   different libraries may freely share an internal helper name (e.g.
   `write_byte` in both `screen` and `debug`) without collision, since each
   is qualified by its own library.

This is why the standard-library and raw-MMIO versions below are
behaviorally identical: the library version is just `use`d s16c source that
got included and qualified by the same rules any of the user's own code
follows.

```rust
// Written against the standard library (LANGUAGE.md §9):
use screen;
use input;
use time;

fn main() {
    loop {
        var keys: u16 = input::read();
        screen::put(20, 12, ' ', screen::BLACK);
        // ...
        time::wait_next();
    }
}
```

```rust
// The same behavior, written directly against MMIO — what screen::put,
// input::read, and time::wait_next compile to underneath. No `use` needed:
// the six hardware builtins (§7.6) are always available.
const FB: u16 = 0xF000;
fn main() {
    loop {
        var keys: u16 = peek(0xFF00);
        poke(FB + (12 * 40 + 20) * 2, 0x0000 | ' ');
        // ...
        var t: u16 = peek(0xFF04);
        while peek(0xFF04) == t { }
    }
}
```

Nothing about `peek`/`poke` (§7.6) changes: the libraries are not a special
compiler feature, they're proof that ordinary s16c is enough to build one.

## String Literals and `str` Parameters (D024)

A string literal (`"..."`) is compile-time data: the compiler emits its
bytes, zero-terminated, into the data section (as `.byte` — not `.asciiz`,
since the assembler's string-escaping only covers ASCII plus the seven
named escapes and can't round-trip arbitrary byte values, while `.byte`
takes any value with no escaping ambiguity) and the argument evaluates to
that data's address. The *token* is legal **only** as a direct argument to
a parameter declared `str` — enforced at parse time (a string token is only
ever consumed while parsing a call's argument list; nowhere else in the
grammar accepts one), not just by the type checker.

Inside the function body, a `str` parameter is an ordinary `u16` — the
compiler doesn't carry any string-specific type past the call site. This
means a `str` *parameter's* argument at a call site isn't restricted to a
literal either: an ordinary `u16` expression type-checks too, which is what
lets one `str`-taking function forward its parameter to another (e.g.
`debug::println(s: str) { debug::print(s); ... }` — `s` inside `println`'s
body is just a `u16` local). Only the literal *token* is call-argument-only;
once you have a `str` value (a parameter), passing it onward is an ordinary
value pass with no special checking, same as any other `u16`.

## CLI Contract

```
s16c <input.s16c> [-o <output>] [--bin]
```

| Behavior | Contract |
|----------|----------|
| Default output | input path with extension replaced: `.s16` (or `.bin` with `--bin`) |
| `--bin` | additionally run the in-process assembler (`s16_asm::assemble`) and write the final image instead of text |
| On success | output written; one summary line on stdout; exit 0 |
| On failure | no output file; diagnostics on stderr; exit 1 |

## Diagnostics

Every error reports `{file}:{line}:{col}: {message}` with a 1-indexed
position. The wording is not contractual; rejecting the condition is. An
implementation must reject at least:

| Class | Examples |
|-------|----------|
| syntax | unbalanced braces, missing `;`, brace-less `if`, a string literal anywhere but a call argument, `str` anywhere but a parameter type |
| name | unknown variable/function/const, duplicate definition **among the user's own items**, use of an array without an index |
| imports | `use` of an unknown library name (message lists the real ones); calling a qualified name that exists in a library the program didn't `use` gets the library-specific hint from Standard Library Merge above, not a bare unknown-name error |
| type | `u16`/`i16` operand mixing without `as`, non-`bool` condition, wrong argument count/types, assigning to a `const`, `return expr` in a `-> ()` function (and vice versa), a non-literal argument where a `str` parameter expects one |
| signedness | `/` or `%` on `i16` (D018), unary `-` on `u16` |
| const | non-constant `static`/`const` initializer, array size not a constant |
| structure | missing `fn main()`, `main` with params/return, `break`/`continue` outside a loop |
| limits | more than 4 parameters is **allowed** (stack args per the ABI) — but a program too large for 60 KB of code+data is an error, not silent truncation |

Warnings (non-fatal, stderr): none required in v1.

## Emitted-Code Contract

1. **Layout:** startup stub at `0x0000` (`CALL main`, then `HALT`), followed
   by function bodies, followed by the data section (`static` words/arrays
   as `.word`, string-literal bytes as `.byte`, all initialized in the
   image — there is no runtime initializer). The whole image loads at
   `0x0000` like any S16 program. Qualified source names (`screen::put`)
   are mangled to legal label text by replacing `::` with `__`
   (`F_screen__put`) — `::` can never appear in a name a program defines
   (LANGUAGE.md §1), so this can't collide with a user-defined label.
2. **ABI:** every compiled function conforms to
   [CALLING_CONVENTION.md](CALLING_CONVENTION.md) v2 — R0–R3 args, R0
   return, callee-saved R4–R11/R15, FP frames via `RSP`/`WSP`, stack args
   beyond four. Leaf functions may use the leaf allowance.
3. **Semantics:** exactly LANGUAGE.md — wrapping arithmetic, signedness of
   comparisons/shifts by type, short-circuit `&&`/`||`, left-to-right
   evaluation, builtins as single instructions.
4. **Not pinned:** register allocation, instruction selection quality,
   label naming, whether a value lives in a register or a stack slot.
   Codegen may improve without a spec change; only behavior is contractual.

## Acceptance Vectors

Behavioral, not byte-for-byte (unlike ASSEMBLY.md's vector — codegen is
allowed to change). Each program must compile, assemble, and run to the
stated machine state. These become the compiler's integration tests, run on
`s16-core`. Results are written to scratch memory at `0x8000+` so they're
observable regardless of register allocation.

**V1 — loops and arithmetic** (sum 1..=10 twice, once by loop, once by formula):

```rust
fn sum_to(n: u16) -> u16 {
    var total: u16 = 0;
    var i: u16 = 1;
    while i <= n { total = total + i; i = i + 1; }
    return total;
}
fn main() { poke(0x8000, sum_to(10)); poke(0x8002, (10 * 11) / 2); halt(); }
```

Required: Halted; `mem16[0x8000] == 55`, `mem16[0x8002] == 55`. (v1 has no
expression-return — bodies end with an explicit `return`.)

**V2 — recursion and the ABI** (`fib(10)` recursively):

```rust
fn fib(n: u16) -> u16 {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}
fn main() { poke(0x8000, fib(10)); halt(); }
```

Required: Halted; `mem16[0x8000] == 55`. Exercises frames, callee-saved
registers, and recursion.

**V3 — statics, arrays, signedness:**

```rust
static A: [u16; 5] = [10, 20, 30, 40, 50];
static ACC: u16 = 0;

fn main() {
    var i: u16 = 0;
    while i < 5 { ACC = ACC + A[i]; i = i + 1; }
    var d: i16 = -3;
    if d < 0 { ACC = ACC + 1; }        // signed compare must be BLT
    poke(0x8000, ACC);
    halt();
}
```

Required: Halted; `mem16[0x8000] == 151`.

**V4 — devices and the standard library** (the §9 walk example from
LANGUAGE.md — `use screen; use input; use time;`, then `screen::put`,
`input::read`, `time::wait_next`, no raw addresses — run under `s16-machine`
for a few ticks with a scripted INPUT): the `@` cell moves right when
`KEY_RIGHT` is held. Pins the imports resolver, module-qualified calls,
builtins, and machine integration together.

## Appendix: Hand-Compiled Reference

`sum_to` from V1, compiled by hand to conforming assembly — the paper
cross-check that the ABI, the language, and the ISA meet. (A real compiler
may emit different but behaviorally identical code.)

```asm
; fn sum_to(n: u16) -> u16     n arrives in R0
; locals: total at [FP-2], i at [FP-4]
sum_to: PUSH R15
        RSP  R15
        SUBI R12, R15, #4      ; two locals
        WSP  R12
        LDI  R12, #0
        STW  [R15-2], R12      ; total = 0
        LDI  R12, #1
        STW  [R15-4], R12      ; i = 1
st_top: LDW  R12, [R15-4]
        CMP  R12, R0           ; flags for i - n
        BLTU st_body           ; i < n  -> body
        BEQ  st_body           ; i == n -> body
        JMP  st_done           ; i > n  -> done
st_body:
        LDW  R12, [R15-2]
        LDW  R13, [R15-4]
        ADD  R12, R12, R13
        STW  [R15-2], R12      ; total = total + i
        LDW  R12, [R15-4]
        ADDI R12, R12, #1
        STW  [R15-4], R12      ; i = i + 1
        JMP  st_top
st_done:
        LDW  R0, [R15-2]       ; return total in R0
        WSP  R15
        POP  R15
        RET
```

(The `i <= n` branch pair shows the general pattern: the ISA has `<`, `>=`,
and `==` branches but no single `<=`/`>` branch, so those comparisons
compile to two branches — or to one branch with swapped operands. The spec
deliberately leaves the choice to the implementation.)
