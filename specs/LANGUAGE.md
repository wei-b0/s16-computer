# s16c — The S16 Language

s16c is a tiny, Rust-flavored language for writing S16 programs — deliberately
just big enough for games like Snake and Pong. It compiles to S16 assembly
([ASSEMBLY.md](ASSEMBLY.md)) via the native compiler
([COMPILER.md](COMPILER.md)), following the ABI in
[CALLING_CONVENTION.md](CALLING_CONVENTION.md).

It borrows Rust's surface (syntax, `let`/`fn`/`loop`, expression feel) without
being Rust: no borrow checker, no references, no heap, no traits. Divergences
from Rust are called out inline.

## 1. Lexical Structure

- **Comments:** `//` to end of line. No block comments.
- **Identifiers:** `[A-Za-z_][A-Za-z0-9_]*`. Case-sensitive.
- **Keywords:** `fn let static const if else while loop break continue return
  true false as u16 i16 bool`
- **Integer literals:** decimal (`42`), hex (`0x2A`), or character (`'A'`,
  with escapes `\n \t \r \0 \\ \' \"`). No suffixes; see §3 for typing.
- **Bool literals:** `true`, `false`.

## 2. Program Structure

A program is a sequence of items in one source file, in any order (no forward
declarations needed):

```rust
const  NAME: u16 = const-expr;          // compile-time constant
static NAME: type = initializer;        // global variable (word or array)
fn name(param: type, ...) -> type { }   // function ('-> type' optional)
```

- **`const`** defines a compile-time name; it occupies no memory and its
  value is folded into every use. Type must be `u16`, `i16`, or `bool`.
- **`static`** defines a global living in the data section of the image.
  *Divergence from Rust:* statics are freely mutable — no `mut`, no `unsafe`.
  Initializers must be constant expressions; array forms are
  `[value; count]` (repeat) or `[a, b, c]` (explicit, length = element count).
- **`fn main()`** — no parameters, no return type — is the entry point and
  must exist. Startup code calls it; if it returns, the CPU halts. A game is
  a `loop { }` inside `main` that never returns.

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
let x: u16 = expr;      // local; ': type' optional when inferable
x = expr;               // assignment to local, param, or static
arr[i] = expr;          // array element assignment
if cond { } else { }    // cond must be bool; 'else if' chains allowed
while cond { }
loop { }                // forever, until break
break; continue;        // innermost while/loop only
return;  return expr;   // expr required iff the fn declares '-> type'
expr;                   // expression statement (calls)
```

Blocks require braces (no brace-less `if`). Locals are scoped to their block
and may shadow outer names. *Divergence from Rust:* locals are mutable
without `mut`, and `if`/blocks are not expressions — there is no
`let x = if ...`.

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
- **No uninitialized values:** `let` and `static` require initializers.
- Recursion is fully supported (frames per the ABI). There is no
  tail-call guarantee.

## 7. Builtin Functions

Compiler-known; they compile to single instructions and follow no ABI:

| Builtin | Effect |
|---------|--------|
| `peek(addr: u16) -> u16` | 16-bit load (`LDW`). Odd `addr` faults the CPU. |
| `poke(addr: u16, value: u16)` | 16-bit store (`STW`). Odd `addr` faults. |
| `peek8(addr: u16) -> u16` | byte load, zero-extended (`LDB`) |
| `poke8(addr: u16, value: u16)` | byte store, low byte (`STB`) |
| `halt()` | stop the CPU (`HALT`); never returns |
| `brk()` | debug breakpoint (`BRK`) |

This is the entire I/O surface: devices are memory
([MEMORY_MAP.md](MEMORY_MAP.md)), so `peek(0xFF00)` reads the keys and
`poke` into `0xF000..` draws.

## 8. Grammar (EBNF)

```ebnf
program    = { item } ;
item       = function | static | const ;
const      = "const" ident ":" scalar "=" cexpr ";" ;
static     = "static" ident ":" type "=" init ";" ;
init       = cexpr | "[" cexpr ";" cexpr "]" | "[" cexpr { "," cexpr } "]" ;
function   = "fn" ident "(" [ param { "," param } ] ")" [ "->" scalar ] block ;
param      = ident ":" scalar ;
scalar     = "u16" | "i16" | "bool" ;
type       = scalar | "[" ( "u16" | "i16" ) ";" cexpr "]" ;

block      = "{" { stmt } "}" ;
stmt       = "let" ident [ ":" scalar ] "=" expr ";"
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

The language-level twin of `programs/spin.s16`/`hello.s16`: walk an `@`
around the screen with the arrow keys, one step per tick.

```rust
// walk.s16c — arrow keys move '@' around the display.

const INPUT: u16 = 0xFF00;
const TICK:  u16 = 0xFF04;
const FB:    u16 = 0xF000;
const COLS:  u16 = 40;
const ROWS:  u16 = 25;

const KEY_UP:    u16 = 1;
const KEY_DOWN:  u16 = 2;
const KEY_LEFT:  u16 = 4;
const KEY_RIGHT: u16 = 8;

static X: u16 = 20;
static Y: u16 = 12;

fn cell(x: u16, y: u16) -> u16 {
    FB + (y * COLS + x) * 2
}

fn wait_next_tick() {
    let t: u16 = peek(TICK);
    while peek(TICK) == t { }
}

fn main() {
    loop {
        let keys: u16 = peek(INPUT);
        poke(cell(X, Y), ' ');                // erase old position
        if keys & KEY_LEFT  != 0 && X > 0        { X = X - 1; }
        if keys & KEY_RIGHT != 0 && X < COLS - 1 { X = X + 1; }
        if keys & KEY_UP    != 0 && Y > 0        { Y = Y - 1; }
        if keys & KEY_DOWN  != 0 && Y < ROWS - 1 { Y = Y + 1; }
        poke(cell(X, Y), 0x0A00 | '@');       // green '@'
        wait_next_tick();
    }
}
```

Everything S16-specific is ordinary code: cell addressing is arithmetic,
input is a `peek`, drawing is a `poke`, pacing is polling `TICK`.

## 10. Snake Sufficiency Sketch

Proof that v1 is enough for the end goal — snake's core state and step logic
in legal v1 s16c (parallel arrays instead of structs, per D015):

```rust
static BODY_X: [u16; 128] = [0; 128];
static BODY_Y: [u16; 128] = [0; 128];
static LEN: u16 = 3;
static DIR: u16 = 0;            // 0=up 1=down 2=left 3=right
static FOOD_X: u16 = 30;
static FOOD_Y: u16 = 10;

fn step() {
    let i: u16 = LEN - 1;       // move body: back to front
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
        FOOD_X = peek(0xFF02) % 40;    // RNG
        FOOD_Y = peek(0xFF02) % 25;
    }
}
```

Input handling, collision (compare head against body cells / walls), and
drawing are the same shapes as §9. Nothing outside v1 is needed.

## Not in v1 (deliberately)

Structs, enums, references/pointers, heap, strings, local/parameter arrays,
signed division, `match`, modules, closures. Candidates for v2 once snake
ships; see [DESIGN_DECISIONS.md](DESIGN_DECISIONS.md) D013–D018.
