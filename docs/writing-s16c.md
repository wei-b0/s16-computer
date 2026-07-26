# Writing s16c Programs

A hands-on tutorial for **s16c**, the tiny Rust-flavored language that
compiles to S16 assembly. Full grammar and semantics live in
[specs/LANGUAGE.md](../specs/LANGUAGE.md); the compiler's own contract
(CLI, diagnostics, what emitted code must do) is
[specs/COMPILER.md](../specs/COMPILER.md).

**s16c is a game language with hardware access available, not a hardware
language you happen to write games in.** Almost everything below is written
against small, explicitly imported standard libraries — `use screen;` gets
you `screen::put`, `use input;` gets you `input::read` — not raw memory
addresses. Nothing from a library is visible until you `use` it, and every
call to it is written out in full: `screen::put(...)`, never a bare `put(...)`.
This is deliberately C's `#include` model plus Rust's fully-qualified paths
(`std::mem::swap(...)` needs no `use` either), not an always-there prelude
and not a `use`-then-bare-name shortcut — so a program's dependencies are
visible both in its `use` lines *and* at every call site that uses them. The
raw addresses are still there if you ever need them (§
[Advanced: talking to hardware directly](#advanced-talking-to-hardware-directly)),
but you won't need them to make Pong.

Compile and run each example with:

```sh
cargo run -p s16c -- yourfile.s16c --bin && cargo run -p s16-emulator -- --tui yourfile.bin
```

`--bin` chains the assembler so you get a runnable image in one step;
`--tui` runs the interactive machine so you can actually watch the screen.
Drop `--bin` and `s16c` emits readable `.s16` assembly text instead — more
on that in [Reading the generated assembly](#reading-the-generated-assembly).

## Your first screen

```rust
use screen;

fn main() {
    screen::clear();
    screen::put(20, 12, '@', screen::GREEN);
    halt();
}
```

Run it with `--tui` and a green `@` sits in the middle of an otherwise
blank 40×25 screen. `use screen;` pulls in the screen library; without that
line, `screen::clear`/`screen::put`/`screen::GREEN` don't exist as far as
the compiler is concerned. Three calls, no addresses: `screen::clear()`
blanks every cell, `screen::put(x, y, glyph, color)` draws one, `halt()`
stops the CPU. `screen::GREEN` is one of nine color constants
(`BLACK GRAY RED GREEN YELLOW BLUE MAGENTA CYAN WHITE`) the library
predefines for you.

`screen::clear`, `screen::put`, `screen::GREEN` — none of these are
keywords. They're ordinary functions and constants from the **screen
library**, one of five standard libraries (more on that in
[The standard library](#the-standard-library)) — always called by their
full `module::name` path, never a bare name.

## Types: u16, i16, bool

Three types, all one 16-bit word at runtime (LANGUAGE.md §3). The type
checker's whole job is picking the right branch/shift instruction —
`u16` comparisons compile to `BLTU`/`BGEU`, `i16` to `BLT`/`BGE`, and `>>`
is logical for `u16` but sign-extending for `i16`.

```rust
fn main() {
    var dx: i16 = -3;
    var steps: u16 = 5;
    poke(0x8000, (dx as u16) + steps);   // 0x0002
    halt();
}
```

(That's `poke` — the low-level escape hatch, covered below — used here
only to park a result somewhere inspectable; see the note in
[Advanced: talking to hardware directly](#advanced-talking-to-hardware-directly)
about how to actually observe it.) `0x8000` ends up holding `0x0002`:
`-3 as u16` is `0xFFFD`, and `0xFFFD + 5` wraps to `2` — all arithmetic
wraps mod 2¹⁶, matching the CPU, with no overflow traps (a deliberate
divergence from Rust's debug-mode panics).

**There's no implicit conversion between `u16` and `i16`.** Mixing them
without an explicit `as` is a compile error:

```
$ s16c bad.s16c
bad.s16c:4:22: expected `u16`, found `i16`
```

for

```rust
fn main() {
    var a: u16 = 1;
    var b: i16 = -1;
    var c: u16 = a + b;   // <- here
    halt();
}
```

Integer literals adapt to whatever type context expects (default `u16` if
none), so `var x: i16 = 10;` just works — the mismatch error only shows up
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
    var total: u16 = 0;
    var i: u16 = 1;
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
level is a compile error:

```
$ s16c bad.s16c
bad.s16c:2:5: `break` outside of a loop
```

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

parses as `(keys & KEY_LEFT) != 0` — what you want. (You'll rarely write
raw bitmask checks like this anyway — see `key_left(keys)` below — but it
matters the moment you touch a bitmask directly.)

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
    var i: u16 = 0;
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
rejected. Also note **locals use `var`, not `let`**: s16c has only one kind
of local and it's always mutable, so there's no `mut` to opt into (D021) —
one deliberate point of surface divergence from Rust, called out so it
never surprises you mid-program.

Array indexing is **not bounds-checked** at runtime — `SCORES[10]` compiles
fine and reads whatever memory the address arithmetic lands on. This is a
sharp edge inherited from having no runtime beyond the CPU itself; keep
loop bounds tied to the array's declared length (as above), and it never
comes up in practice.

## The standard library

Everything you've used so far except `poke`/`halt` — `screen::clear`,
`screen::put`, `screen::GREEN` — comes from the **standard library**: five
small, separately importable libraries for screen, input, time, randomness,
and debug output (LANGUAGE.md §7), written in s16c itself. `use` the ones
you need, and always call their items by the full `module::name` path —
there's no bare-name shortcut, even after `use`.

### `use screen;`

| Call | Effect |
|------|--------|
| `screen::SCREEN_W`, `screen::SCREEN_H` | `40`, `25` — the display size |
| `screen::put(x, y, glyph, color)` | draw one character cell |
| `screen::get(x, y) -> u16` | read back the glyph at `(x, y)` — for collision checks |
| `screen::clear()` | blank the whole screen |
| `screen::print(x, y, s, color)` | draw a string, one glyph per cell, left to right |
| `screen::BLACK GRAY RED GREEN YELLOW BLUE MAGENTA CYAN WHITE` | color constants |

### `use input;`

| Call | Effect |
|------|--------|
| `input::read() -> u16` | the current key bitmask (never blocks) |
| `input::key_up/down/left/right/a/b/start/select(keys) -> bool` | test one key from an `input::read()` bitmask |
| `input::wait_key() -> u16` | block until a key is held, then return the bitmask |

### `use time;`

| Call | Effect |
|------|--------|
| `time::tick() -> u16` | the machine's current tick count |
| `time::wait_next()` | block until the next tick — the standard way to pace a game loop |

### `use rng;`

| Call | Effect |
|------|--------|
| `rng::next(max) -> u16` | a pseudorandom value in `0..max` |

### `use debug;`

Text output through a debug-console MMIO device — *not* the framebuffer, so
it doesn't disturb anything you've drawn — and host-agnostic: the local
emulator prints it, a future Solana host could map it to program logs or
ignore it. Unlike the others, this one takes **string literals**, covered
just below.

| Call | Effect |
|------|--------|
| `debug::print(s)` | write a string |
| `debug::println(s)` | write a string, then a newline |
| `debug::print_u16(value)` | write a `u16` in decimal |
| `debug::print_i16(value)` | write an `i16` in decimal, with a `-` if negative |
| `debug::print_hex(value)` | write a `u16` as `0x` + 4 hex digits |

```rust
use debug;

fn main() {
    debug::print_u16(150);      // "150"
    debug::println("done");     // "done\n"
    halt();
}
```

Run this in **batch mode** — no `--tui` needed:

```sh
cargo run -p s16c -- yourfile.s16c --bin
cargo run -p s16-emulator -- yourfile.bin
```

and `150` and `done` show up right in the terminal, above the usual register
dump. This is the fix for the thing raw `peek`/`poke` could never quite do:
get a value out of a simple, non-interactive program without drawing to the
screen or reading memory from Rust yourself.

**String literals (`"..."`) only work as arguments to `debug::print`/
`println`/`screen::print`.** There's no general string type — you can't
assign one to a `var`, compare two, or pass one where a plain `u16` is
expected:

```
$ s16c bad.s16c
bad.s16c:2:14: expected an expression
```

for `var x: u16 = "hi";` — a string literal simply isn't valid syntax
outside a call argument, so this is a parse error before the type checker
even runs. It's compile-time data (bytes baked into the image) and an
address, nothing more (D024) — the same "no heap, nothing runtime" spirit
as the rest of the language.

Put together, this is enough for a complete interactive program — no raw
addresses anywhere:

```rust
use screen;
use input;
use time;

static X: u16 = 20;
static Y: u16 = 12;

fn main() {
    screen::put(X, Y, '@', screen::GREEN);                    // draw the start position
    loop {
        var keys: u16 = input::wait_key();                    // park until a key is held
        screen::put(X, Y, ' ', screen::BLACK);                // erase old position
        if input::key_left(keys)  && X > 0                    { X = X - 1; }
        if input::key_right(keys) && X < screen::SCREEN_W - 1  { X = X + 1; }
        if input::key_up(keys)    && Y > 0                    { Y = Y - 1; }
        if input::key_down(keys)  && Y < screen::SCREEN_H - 1  { Y = Y + 1; }
        screen::put(X, Y, '@', screen::GREEN);
        time::wait_next();
    }
}
```

This is [programs/s16c/walk.s16c](../programs/s16c/walk.s16c) — a `loop { }`
that blocks on `input::wait_key()`, redraws, and paces itself on
`time::wait_next()` *is* a game loop, the same shape snake will use. Between
keypresses the CPU is genuinely parked (`WAIT`, D026/D027) — not spinning —
so idle frames barely register on the emulator's instructions-per-second
figures:

```sh
cargo run -p s16c -- programs/s16c/walk.s16c --bin
cargo run -p s16-emulator -- --tui programs/s16c/walk.bin   # arrow keys move '@'
```

**Forget a `use` and the compiler tells you which one:**

```
$ s16c bad.s16c
bad.s16c:2:5: unknown function `screen::put` (found in library `screen` — add `use screen;`)
```

That's a call to `screen::put` with no `use screen;` anywhere in the file —
the error names the missing library instead of just saying "unknown
function." (This hint only fires for a *qualified* call, `module::name(...)`
— a bare, unqualified name is never a library reference at all, qualified
or not, so it just falls through to an ordinary unknown-name error.) `use`
an unknown library name and you get a similar nudge:

```
$ s16c bad.s16c
bad.s16c:1:5: no such library `graphics`; available: debug, input, rng, screen, time
```

**`use` order doesn't matter**, same as every other item — it can go at the
top of the file (the convention above) or anywhere else. Repeating a `use`
for a library you already imported is harmless, not an error.

**Only what you reference gets compiled in**, even from a `use`d library.
The library is ordinary s16c code; the compiler includes a function only if
your program calls it (directly or transitively — calling `screen::put`
pulls in the small helper it uses internally, for instance), so
`use screen;` doesn't bloat your image with every screen function you
didn't call.

**No package manager.** These five libraries are the entire built-in
catalog — there's no mechanism yet for pulling in a file, a URL, or a
third-party crate-equivalent. If you need something the libraries don't
offer, write it in your own program (or reach for
[Advanced: talking to hardware directly](#advanced-talking-to-hardware-directly)).

## Advanced: talking to hardware directly

Everything in the standard library is built from seven lower-level
builtins — `peek`/`poke` (word) and `peek8`/`poke8` (byte), plus
`halt`/`brk`/`wait`. Reach for these when you need an address the library
doesn't have a name for yet; `screen::put` itself is just a couple of
multiplies and a `poke`, and `input::wait_key()` is `read()` plus a `wait()`.

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

This draws the same way `screen::put` does, just spelling out the
framebuffer address by hand instead of calling it — a good exercise once,
and the pattern to reach for later if you outgrow what §
[The standard library](#the-standard-library) offers.

A practical note: before `debug::print*` existed, `poke`'d results were
awkward to inspect — they just sit in memory, and the batch emulator's
register dump won't show them. `use debug;` and `debug::print_u16`/etc.
(above) is the normal way to check a value now. The old ways still work
too: poke it onto the framebuffer and watch with `--tui`, or inspect memory
directly from Rust the way the compiler's own test suite does (see
[using-s16-core.md](using-s16-core.md#writing-tests-against-the-cpu)):

```rust
let image = s16_asm::assemble(&s16c::compile(source)?)?;
// ...load into memory, cpu.run(...), then read mem[0x8000..0x8002]
```

## Reading the generated assembly

Drop `--bin` and `s16c` writes `.s16` text instead of a binary image:

```sh
cargo run -p s16c -- yourfile.s16c    # writes yourfile.s16
```

The output is deliberately readable — every function becomes a labeled
`F_name:` block with a standard prologue, every `static` becomes a `.word`
line under a `G_name:` label, every string literal a `.byte` line — so when
something compiles to behavior you didn't expect, reading the `.s16` is the
fastest way to find out why. Codegen is intentionally simple rather than
clever (COMPILER.md pins *behavior*, not code quality), so the output is
usually easy to follow even without deep assembly experience: expressions
evaluate through `R12`/`R13`, locals live at `[R15-2]`, `[R15-4]`, ... and
so on. Calling a standard library function like `screen::put` looks exactly
like calling one of your own (label `F_screen__put` — `::` becomes `__`,
since assembly labels can't contain `::`) — because to the compiler, it is
one.

You can also assemble and run that `.s16` file exactly like any hand-written
program from [writing-programs.md](writing-programs.md) — it's ordinary S16
assembly, nothing about it is special-cased.

## Errors

Every diagnostic is `file:line:col: message` — the compiler never leaves
behind a stale output file when it fails. A few more you'll run into
early on:

- **`unknown variable` / `unknown function` / `unknown name`** — typo, or
  using a name before its scope. Unlike items (`fn`/`static`/`const`),
  which can appear in any order, locals must be declared with `var` before
  use.
- **`` `f` expects N argument(s), got M ``** — arity is checked at every
  call site, builtins and library calls included (`screen::put` takes
  exactly four arguments).
- **`expected a string literal`** — a `str` parameter (`debug::print`,
  `screen::print`) was called with something other than a literal `"..."`
  directly in the call — a `var` holding text isn't possible to construct
  in the first place (D024), so this only ever fires on things like passing
  a `u16` where a string was expected.
- **`cannot assign to constant`** — `const` is compile-time only and holds
  no memory; only `static` is assignable.
- **`comparisons are non-associative; parenthesize`** — `a < b < c` isn't
  valid (it isn't in Rust either); write `a < b && b < c`.

If an error's wording is ever confusing, the underlying rule is always in
[specs/LANGUAGE.md](../specs/LANGUAGE.md) or the diagnostics table in
[specs/COMPILER.md](../specs/COMPILER.md#diagnostics) — the message text
itself isn't part of the contract, but what gets rejected is.

## What's not here yet

Structs, enums, pointers, a general string type, local arrays, `match`,
closures, user-authored or external libraries, and user-defined modules —
all explicitly deferred past v1 (LANGUAGE.md's closing section). String
*literals* are in v1, but only as call arguments (`str` parameters,
above) — there's still no way to hold, build, or manipulate text at
runtime. `use` only names one of the five built-in libraries; there's no
file-path or URL form, and no `use module::item;` single-item form — only
`use module;` then the full `module::name` path. There are also no methods
or dot syntax: the standard library is called with `::`, not `.`
(`screen::put(x, y, ...)`, not `screen.put(x, y, ...)`). If a program needs
a struct, the workaround today is usually parallel arrays (as
`SCORES`/`TOTAL` above) or plain integers standing in for an enum. That's
enough for Snake and Pong, which is the whole design point.
