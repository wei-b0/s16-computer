# s16c — The S16 Language

s16c is a tiny, Rust-flavored language for writing S16 programs — deliberately
just big enough for games like Snake and Pong. It compiles to S16 assembly
([ASSEMBLY.md](ASSEMBLY.md)) via the native compiler
([COMPILER.md](COMPILER.md)), following the ABI in
[CALLING_CONVENTION.md](CALLING_CONVENTION.md).

It borrows Rust's surface (syntax, `fn`/`loop`, expression feel) without
being Rust: no borrow checker, no references, no heap, no traits. Divergences
from Rust are called out inline.

**s16c is a tiny game language with low-level hardware access available, not
a low-level language that happens to support games (D019).** Application
code is written against the standard library in §7 — `put_cell`, `read_input`,
`wait_tick`, and friends, pulled in explicitly with `use screen;` and
friends (D022) — not raw MMIO addresses. `peek`/`poke` still exist and still
work everywhere, needing no `use`, but they are the advanced escape hatch,
not the primary API; see the worked example in §9.

## 1. Lexical Structure

- **Comments:** `//` to end of line. No block comments.
- **Identifiers:** `[A-Za-z_][A-Za-z0-9_]*`. Case-sensitive.
- **Keywords:** `fn var static const use if else while loop break continue
  return true false as u16 i16 bool`
- **Integer literals:** decimal (`42`), hex (`0x2A`), or character (`'A'`,
  with escapes `\n \t \r \0 \\ \' \"`). No suffixes; see §3 for typing.
- **Bool literals:** `true`, `false`.

## 2. Program Structure

A program is a sequence of items in one source file, in any order (no forward
declarations needed):

```rust
use    NAME;                            // pull in a standard library (§7)
const  NAME: u16 = const-expr;          // compile-time constant
static NAME: type = initializer;        // global variable (word or array)
fn name(param: type, ...) -> type { }   // function ('-> type' optional)
```

- **`use`** pulls in one of the fixed, built-in standard libraries by name
  (§7) — `use screen;`. Without it, that library's functions, statics, and
  constants are not visible at all: calling `put_cell` without `use screen;`
  is a compile error, not an implicit include. Order among items doesn't
  matter, so a `use` may appear anywhere in the file, same as any other item.
- **`const`** defines a compile-time name; it occupies no memory and its
  value is folded into every use. Type must be `u16`, `i16`, or `bool`.
- **`static`** defines a global living in the data section of the image.
  *Divergence from Rust:* statics are freely mutable — no `mut`, no `unsafe`.
  Initializers must be constant expressions; array forms are
  `[value; count]` (repeat) or `[a, b, c]` (explicit, length = element count).
- **`fn main()`** — no parameters, no return type — is the entry point and
  must exist. Startup code calls it; if it returns, the CPU halts. A game is
  a `loop { }` inside `main` that never returns.

A program's own `fn`/`static`/`const` definition shadows a used library's
item of the same name completely — see §7 for the exact rule.

## 3. Types

| Type | Meaning |
|------|---------|
| `u16` | unsigned 16-bit integer, wrapping arithmetic |
| `i16` | signed 16-bit integer (two's complement), wrapping arithmetic |
| `bool` | `true`/`false` |
| `[u16; N]`, `[i16; N]` | fixed-size array — **static items only** (no array locals or parameters in v1) |

Everything is one 16-bit word at runtime (`bool` is 0 or 1). The type system
exists at compile time only, and its one hard job is signedness: `u16`
comparisons compile to `BLTU`/`BGEU`, `i16` comparisons to `BLT`/`BGE`.

- **No implicit conversions.** Mixing `u16` and `i16` operands is an error;
  convert explicitly with `as`.
- **`as` casts:** `expr as u16` / `expr as i16` between the two integer
  types. Bit-for-bit reinterpretation, zero runtime cost. `bool` does not
  cast in v1.
- **Literal typing:** an integer literal adapts to the type expected by
  context if it fits; otherwise it defaults to `u16`. `-1` (unary minus on a
  literal) is an `i16` literal.

## 4. Statements

```rust
var x: u16 = expr;      // local; ': type' optional when inferable
x = expr;                // assignment to local, param, or static
arr[i] = expr;            // array element assignment
if cond { } else { }    // cond must be bool; 'else if' chains allowed
while cond { }
loop { }                // forever, until break
break; continue;        // innermost while/loop only
return;  return expr;   // expr required iff the fn declares '-> type'
expr;                   // expression statement (calls)
```

Blocks require braces (no brace-less `if`). Locals are scoped to their block
and may shadow outer names. *Divergence from Rust:* locals are declared with
`var`, not `let`/`let mut` — s16c has only one kind of local, and it is
always mutable, so there is no `mut` ceremony to learn. `if`/blocks are not
expressions — there is no `var x = if ...`.

## 5. Expressions

Operators, tightest first (Rust's precedence, **not** C's — note that `&`
binds tighter than `==`, so `keys & MASK != 0` means `(keys & MASK) != 0`):

| Level | Operators | Notes |
|-------|-----------|-------|
| unary | `-` `!` | `-` on `i16` only; `!` is logical on `bool`, bitwise NOT on integers |
| cast | `as` | |
| mul | `*` `/` `%` | `/` `%` on `u16` only (§6) |
| add | `+` `-` | |
| shift | `<<` `>>` | `>>` is logical for `u16`, arithmetic for `i16`; shift count is `u16` |
| bitand | `&` | integers |
| bitxor | `^` | integers |
| bitor | `\|` | integers |
| compare | `==` `!=` `<` `<=` `>` `>=` | both operands same type; result `bool`; non-associative (`a < b < c` is an error) |
| and | `&&` | `bool`, short-circuit |
| or | `\|\|` | `bool`, short-circuit |

Primary expressions: literals, names, `arr[index]` (index is `u16`), calls
`f(a, b)`, parentheses. Argument and operand evaluation is left-to-right.

## 6. Semantics

- **Arithmetic wraps** mod 2¹⁶ — no overflow traps (*divergence from Rust's
  debug overflow panics*; matches the CPU).
- **`/` and `%` are `u16`-only** in v1: the CPU divides unsigned
  ([INSTRUCTION_SET.md](INSTRUCTION_SET.md#alu--register)); signed division
  would need a library routine (D018). Division by zero faults the CPU at
  runtime (DIV_BY_ZERO) — the compiler does not guard it.
- **Array indexing is not bounds-checked** at runtime in v1. An
  out-of-range index reads/writes whatever memory the address arithmetic
  lands on. (*Divergence from Rust* — documented sharp edge.)
- **No uninitialized values:** `var` and `static` require initializers.
- Recursion is fully supported (frames per the ABI). There is no
  tail-call guarantee.

## 7. Standard Library

The platform API — the primary way programs touch the screen, input, timing,
and randomness. It is written in s16c itself, as four separate libraries
(`crates/s16c/src/libs/*.s16c`), and a program opts into each one it needs
with `use` (D019, D022):

- **Nothing is visible without `use`.** `put_cell` is undefined until the
  program writes `use screen;`; calling it before that is a compile error
  (with a hint naming the library to `use`, if the compiler recognizes the
  name from *some* library). This is the whole point of the library split —
  a program's dependencies are visible in its source, the same way C's
  `#include` makes them visible.
- **Only what you use is included in the image**, even within a `use`d
  library: the compiler includes a function/static only if the program
  references it, directly or transitively (e.g. calling `put_cell` pulls in
  the small helper it uses internally). `use screen;` does not bloat the
  image with every screen function you didn't call.
- **Your definitions win.** A program that defines its own `fn`/`static`/
  `const` with the same name as a `use`d library's item shadows it
  completely — every reference to that name, including from inside other
  library functions, resolves to your version.
- **Fixed set, no package manager.** The four libraries below are the
  entire catalog — there's no mechanism for external files, third-party
  libraries, or user-authored ones in v1.

### 7.1 `use screen;`

40×25 cells, matching the framebuffer described in
[MEMORY_MAP.md](MEMORY_MAP.md).

| Item | Effect |
|------|--------|
| `SCREEN_W: u16`, `SCREEN_H: u16` | `40`, `25` |
| `put_cell(x: u16, y: u16, glyph: u16, color: u16)` | draw one character cell |
| `get_cell(x: u16, y: u16) -> u16` | read back the glyph at `(x, y)` (for collision checks) |
| `clear_screen()` | blank the whole screen |
| `BLACK GRAY RED GREEN YELLOW BLUE MAGENTA CYAN WHITE: u16` | color constants for `put_cell`'s `color` argument |

### 7.2 `use input;`

| Item | Effect |
|------|--------|
| `read_input() -> u16` | the current key bitmask |
| `key_up/down/left/right/a/b/start/select(keys: u16) -> bool` | test one bit of a bitmask from `read_input()` |

### 7.3 `use time;`

| Item | Effect |
|------|--------|
| `ticks() -> u16` | the machine's current tick count |
| `wait_tick()` | block until the next tick — the standard way to pace a game loop |

### 7.4 `use random;`

| Item | Effect |
|------|--------|
| `random(max: u16) -> u16` | a pseudorandom value in `0..max` (`0` if `max` is `0`) |

### 7.5 Hardware Access (Advanced)

The four libraries above are built entirely out of these; reach for them
only when you need an address no library names yet. Unlike the libraries,
these need no `use` — they are core language builtins, always available.
They compile to a single instruction each and follow no calling convention:

| Builtin | Effect |
|---------|--------|
| `peek(addr: u16) -> u16` | 16-bit load (`LDW`). Odd `addr` faults the CPU. |
| `poke(addr: u16, value: u16)` | 16-bit store (`STW`). Odd `addr` faults. |
| `peek8(addr: u16) -> u16` | byte load, zero-extended (`LDB`) |
| `poke8(addr: u16, value: u16)` | byte store, low byte (`STB`) |
| `halt()` | stop the CPU (`HALT`); never returns |
| `brk()` | debug breakpoint (`BRK`) |

Devices are memory ([MEMORY_MAP.md](MEMORY_MAP.md)), so these are how the
libraries themselves are written — `put_cell` is a couple of arithmetic ops
and a `poke`, nothing more.

## 8. Grammar (EBNF)

```ebnf
program    = { item } ;
item       = function | static | const | use ;
use        = "use" ident ";" ;
const      = "const" ident ":" scalar "=" cexpr ";" ;
static     = "static" ident ":" type "=" init ";" ;
init       = cexpr | "[" cexpr ";" cexpr "]" | "[" cexpr { "," cexpr } "]" ;
function   = "fn" ident "(" [ param { "," param } ] ")" [ "->" scalar ] block ;
param      = ident ":" scalar ;
scalar     = "u16" | "i16" | "bool" ;
type       = scalar | "[" ( "u16" | "i16" ) ";" cexpr "]" ;

block      = "{" { stmt } "}" ;
stmt       = "var" ident [ ":" scalar ] "=" expr ";"
           | place "=" expr ";"
           | "if" expr block [ "else" ( block | ifstmt ) ]
           | "while" expr block
           | "loop" block
           | "break" ";" | "continue" ";"
           | "return" [ expr ] ";"
           | expr ";" ;
place      = ident [ "[" expr "]" ] ;

expr       = orexpr ;
orexpr     = andexpr { "||" andexpr } ;
andexpr    = cmpexpr { "&&" cmpexpr } ;
cmpexpr    = bitor [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) bitor ] ;
bitor      = bitxor { "|" bitxor } ;
bitxor     = bitand { "^" bitand } ;
bitand     = shift { "&" shift } ;
shift      = addexpr { ( "<<" | ">>" ) addexpr } ;
addexpr    = mulexpr { ( "+" | "-" ) mulexpr } ;
mulexpr    = castexpr { ( "*" | "/" | "%" ) castexpr } ;
castexpr   = unary { "as" ( "u16" | "i16" ) } ;
unary      = ( "-" | "!" ) unary | postfix ;
postfix    = primary { "[" expr "]" | "(" [ expr { "," expr } ] ")" } ;
primary    = int | char | "true" | "false" | ident | "(" expr ")" ;

cexpr      = (* expr restricted to literals, const names, and
                integer/bitwise operators, folded at compile time *) ;
```

## 9. Worked Example

Walk an `@` around the screen with the arrow keys, one step per tick —
written entirely against the standard library from §7, with no raw
addresses anywhere:

```rust
// walk.s16c — arrow keys move '@' around the display.

use screen;
use input;
use time;

static X: u16 = 20;
static Y: u16 = 12;

fn main() {
    loop {
        var keys: u16 = read_input();
        put_cell(X, Y, ' ', BLACK);             // erase old position
        if key_left(keys)  && X > 0             { X = X - 1; }
        if key_right(keys) && X < SCREEN_W - 1  { X = X + 1; }
        if key_up(keys)    && Y > 0             { Y = Y - 1; }
        if key_down(keys)  && Y < SCREEN_H - 1  { Y = Y + 1; }
        put_cell(X, Y, '@', GREEN);
        wait_tick();
    }
}
```

Compare this with the equivalent written directly against MMIO in
[COMPILER.md](COMPILER.md) — same behavior, but every address, bit position,
and attribute byte is spelled out by hand. The standard library exists
precisely so application code doesn't have to look like that.

## 10. Snake Sufficiency Sketch

Proof that v1 is enough for the end goal — snake's core state and step logic
in legal v1 s16c (parallel arrays instead of structs, per D015):

```rust
use screen;
use random;

static BODY_X: [u16; 128] = [0; 128];
static BODY_Y: [u16; 128] = [0; 128];
static LEN: u16 = 3;
static DIR: u16 = 0;            // 0=up 1=down 2=left 3=right
static FOOD_X: u16 = 30;
static FOOD_Y: u16 = 10;

fn step() {
    var i: u16 = LEN - 1;       // move body: back to front
    while i > 0 {
        BODY_X[i] = BODY_X[i - 1];
        BODY_Y[i] = BODY_Y[i - 1];
        i = i - 1;
    }
    if DIR == 0 { BODY_Y[0] = BODY_Y[0] - 1; }
    if DIR == 1 { BODY_Y[0] = BODY_Y[0] + 1; }
    if DIR == 2 { BODY_X[0] = BODY_X[0] - 1; }
    if DIR == 3 { BODY_X[0] = BODY_X[0] + 1; }

    if BODY_X[0] == FOOD_X && BODY_Y[0] == FOOD_Y {
        LEN = LEN + 1;
        FOOD_X = random(SCREEN_W);
        FOOD_Y = random(SCREEN_H);
    }
}
```

Input handling (`read_input`/`key_*`), collision (`get_cell` against body
cells / walls), and drawing (`put_cell`) are the same shapes as §9. Nothing
outside v1 — language or library — is needed.

## Not in v1 (deliberately)

Structs, enums, references/pointers, heap, strings, local/parameter arrays,
signed division, `match`, user-authored/external libraries, closures.
Candidates for v2 once snake ships; see
[DESIGN_DECISIONS.md](DESIGN_DECISIONS.md) D013–D022.
