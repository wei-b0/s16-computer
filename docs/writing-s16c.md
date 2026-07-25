# Writing s16c Programs

A hands-on tutorial for **s16c**, the tiny Rust-flavored language that
compiles to S16 assembly. Full grammar and semantics live in
[specs/LANGUAGE.md](../specs/LANGUAGE.md); the compiler's own contract
(CLI, diagnostics, what emitted code must do) is
[specs/COMPILER.md](../specs/COMPILER.md). This page builds programs the way
you actually write them. If you haven't written raw S16 assembly yet,
[writing-programs.md](writing-programs.md) is worth a skim first — s16c
compiles down to exactly what that page teaches, and knowing the target
makes the compiler's behavior predictable rather than magic.

Compile and run each example with:

```sh
cargo run -p s16c -- yourfile.s16c --bin && cargo run -p s16-emulator -- yourfile.bin
```

`--bin` chains the assembler so you get a runnable image in one step. Drop
`--bin` and `s16c` emits readable `.s16` assembly text instead — useful any
time you want to see exactly what your program compiled to (more on that in
[Reading the generated assembly](#reading-the-generated-assembly)).

## The smallest program

```rust
fn main() {
    poke(0x8000, 42);
    halt();
}
```

Every program needs `fn main()` — no parameters, no return type — and it's
where execution begins. `poke`/`peek` are the language's entire I/O surface
(§7 below); there's no `println!`, because there's nowhere for text to go
except memory. `halt()` stops the CPU, same as the assembly `HALT`.

Compile it and the batch emulator confirms a clean stop:

```
halted after 10 instructions
```

Notice batch mode's register dump won't show you `0x8000` — it only prints
registers, and s16c keeps everything in memory or on the stack, never
parked in a register for you to find. To actually see a result, either poke
it onto the **framebuffer** and watch with `--tui` (the natural way once you
have a screen — §7), or check memory directly the way the compiler's own
test suite does, from Rust (see
[using-s16-core.md](using-s16-core.md#writing-tests-against-the-cpu)):

```rust
let image = s16_asm::assemble(&s16c::compile(source)?)?;
// ...load into memory, cpu.run(...), then read mem[0x8000..0x8002]
```

Keep that trick in your pocket — the rest of this guide uses it implicitly
whenever an example "produces" a value.

## Types: u16, i16, bool

Three types, all one 16-bit word at runtime (LANGUAGE.md §3). The type
checker's whole job is picking the right branch/shift instruction —
`u16` comparisons compile to `BLTU`/`BGEU`, `i16` to `BLT`/`BGE`, and `>>`
is logical for `u16` but sign-extending for `i16`.

```rust
fn main() {
    let dx: i16 = -3;
    let steps: u16 = 5;
    poke(0x8000, (dx as u16) + steps);   // 0x0002
    halt();
}
```

`0x8000` holds `0x0002`: `-3 as u16` is `0xFFFD`, and `0xFFFD + 5` wraps to
`2` — all arithmetic wraps mod 2¹⁶, matching the CPU, with no overflow traps
(a deliberate divergence from Rust's debug-mode panics).

**There's no implicit conversion between `u16` and `i16`.** Mixing them
without an explicit `as` is a compile error:

```
$ s16c bad.s16c
bad.s16c:4:22: expected `u16`, found `i16`
```

for

```rust
fn main() {
    let a: u16 = 1;
    let b: i16 = -1;
    let c: u16 = a + b;   // <- here
    halt();
}
```

Integer literals adapt to whatever type context expects (default `u16` if
none), so `let x: i16 = 10;` just works — the mismatch error only shows up
when two *typed* values of different signedness meet.

Division and modulo (`/`, `%`) are **`u16`-only** in v1 — the CPU's `DIV`
only divides unsigned, and signed division would need a software routine
(D018 in [DESIGN_DECISIONS.md](../specs/DESIGN_DECISIONS.md)):

```
$ s16c bad.s16c
bad.s16c:3:20: `/` and `%` are `u16`-only in v1 (D018)
```

## Control flow

`if`/`else`, `while`, and `loop` — all Rust-shaped, but blocks always need
braces (no bare `if cond stmt;`) and there's no `if` used as an expression:

```rust
fn main() {
    let total: u16 = 0;
    let i: u16 = 1;
    while i <= 10 {
        total = total + i;
        i = i + 1;
    }
    poke(0x8000, total);   // 55
    halt();
}
```

`loop { }` runs forever until `break`; `continue` skips to the next
iteration. Both only make sense inside a loop — using either at the top
level is a compile error (`break outside of a loop`), same as Rust.

Conditions must be `bool` — there's no C-style "any nonzero value is
truthy":

```
$ s16c bad.s16c
bad.s16c:3:8: expected `bool`, found `u16`
```

for `if x { halt(); }` where `x: u16`. Compare explicitly: `if x != 0 { }`.

One precedence trap worth knowing up front, because it bites Rust and C
programmers alike: `&` binds **tighter** than `!=`/`==` (Rust's rule, not
C's), so

```rust
if keys & KEY_LEFT != 0 { ... }
```

parses as `(keys & KEY_LEFT) != 0` — what you want. If you ever need the
other grouping, add parens.

## Functions and recursion

```rust
fn fib(n: u16) -> u16 {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}

fn main() {
    poke(0x8000, fib(10));   // 55
    halt();
}
```

Every function compiles to a full stack frame per
[CALLING_CONVENTION.md](../specs/CALLING_CONVENTION.md) — arguments in
R0–R3 (stack-passed beyond four), locals addressed off a frame pointer,
callee-saved registers preserved. That frame is what makes recursion work
by construction: `fib` calling itself just stacks another frame, no special
casing needed. You never write any of this by hand; it's what `--bin`
assembles for you.

Two rules the checker enforces:

- **A function declaring `-> T` must return on every path.** Falling off
  the end without a `return` is an error:
  ```
  f.s16c:1:4: `f` may finish without returning a value
  ```
  (There's no implicit "last expression is the return value" — unlike
  Rust, s16c has no expression-oriented blocks. Every return is explicit.)
- Argument types and count are checked like any call: wrong arity or a
  type mismatch is caught at compile time, not left to fault at runtime.

## Statics and arrays

No heap in s16c — global state lives in `static` words and fixed-size
`static` arrays (LANGUAGE.md's deliberate v1 scope: parallel arrays instead
of structs, since there's no struct type yet):

```rust
static SCORES: [u16; 5] = [10, 20, 30, 40, 50];
static TOTAL: u16 = 0;

fn main() {
    let i: u16 = 0;
    while i < 5 {
        TOTAL = TOTAL + SCORES[i];
        i = i + 1;
    }
    poke(0x8000, TOTAL);   // 150
    halt();
}
```

*Divergence from Rust:* statics are freely mutable, no `mut`, no `unsafe` —
matching S16's memory model where everything is already one big mutable
address space (D001). Initializers must be constant expressions (literals,
`const`s, arithmetic on them) — a function call in a static initializer is
rejected:

```
$ s16c bad.s16c
bad.s16c:2:19: not a constant expression
```

Array indexing is **not bounds-checked** at runtime — `SCORES[10]` compiles
fine and reads whatever memory the address arithmetic lands on. This is a
sharp edge inherited from having no runtime beyond the CPU itself; keep
loop bounds tied to the array's declared length (as above), and it never
comes up in practice.

## Talking to the hardware

`peek`/`poke` (word) and `peek8`/`poke8` (byte) are the entire I/O surface —
no pointer types exist in s16c at all, just these six builtins
(LANGUAGE.md §7). Devices are memory
([specs/MEMORY_MAP.md](../specs/MEMORY_MAP.md)), so this is genuinely
enough: reading `INPUT`, writing the framebuffer, seeding from `RNG`, are
all `peek`/`poke` calls at the right address.

Character literals (`'H'`) make writing to the display straightforward —
there are no string literals in v1, so text is one `poke` per cell:

```rust
const FB: u16 = 0xF000;

fn put(col: u16, ch: u16) {
    poke(FB + col * 2, 0x0F00 | ch);   // attribute 0x0F, character ch
}

fn main() {
    put(0, 'H');
    put(1, 'I');
    halt();
}
```

Run this with `--tui` and `HI` appears at the top-left of the screen —
same framebuffer format as the assembly-level examples in
[writing-programs.md](writing-programs.md#talking-to-the-hardware), just
addressed with ordinary function calls instead of hand-computed offsets.

For a complete interactive program — reading `INPUT`, pacing on `TICK`,
never halting — see [programs/walk.s16c](../programs/walk.s16c), the
worked example from LANGUAGE.md §9:

```sh
cargo run -p s16c -- programs/walk.s16c --bin
cargo run -p s16-emulator -- --tui programs/walk.bin   # arrow keys move '@'
```

A `loop { }` that polls `INPUT` and redraws each tick *is* a game loop —
that's the whole shape snake will use.

## Reading the generated assembly

Drop `--bin` and `s16c` writes `.s16` text instead of a binary image:

```sh
cargo run -p s16c -- yourfile.s16c    # writes yourfile.s16
```

The output is deliberately readable — every function becomes a labeled
`F_name:` block with a standard prologue, every `static` becomes a `.word`
line under a `G_name:` label — so when something compiles to behavior you
didn't expect, reading the `.s16` is the fastest way to find out why.
Codegen is intentionally simple rather than clever (COMPILER.md pins
*behavior*, not code quality), so the output is usually easy to follow even
without deep assembly experience: expressions evaluate through `R12`/`R13`,
locals live at `[R15-2]`, `[R15-4]`, ... and so on.

You can also assemble and run that `.s16` file exactly like any hand-written
program from [writing-programs.md](writing-programs.md) — it's ordinary S16
assembly, nothing about it is special-cased.

## Errors

Every diagnostic is `file:line:col: message` — the compiler never leaves
behind a stale output file when it fails. A few more you'll run into
early on:

- **`unknown variable` / `unknown function` / `unknown name`** — typo, or
  using a name before its scope. Unlike items (`fn`/`static`/`const`),
  which can appear in any order, locals must be declared with `let` before
  use.
- **`` `f` expects N argument(s), got M ``** — arity is checked at every
  call site, builtins included (`peek` takes exactly one address).
- **`cannot assign to constant`** — `const` is compile-time only and holds
  no memory; only `static` is assignable.
- **`comparisons are non-associative; parenthesize`** — `a < b < c` isn't
  valid (it isn't in Rust either); write `a < b && b < c`.

If an error's wording is ever confusing, the underlying rule is always in
[specs/LANGUAGE.md](../specs/LANGUAGE.md) or the diagnostics table in
[specs/COMPILER.md](../specs/COMPILER.md#diagnostics) — the message text
itself isn't part of the contract, but what gets rejected is.

## What's not here yet

Structs, enums, pointers, strings, local arrays, `match`, closures — all
explicitly deferred past v1 (LANGUAGE.md's closing section). If a program
needs one of these, the workaround today is usually parallel arrays (as
`SCORES`/`TOTAL` above) or plain integers standing in for an enum. That's
enough for Snake and Pong, which is the whole design point.
