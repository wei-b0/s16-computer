# Getting Started

Run your first S16 program in about two minutes — in **s16c**, the language
you'll use for everything from here on. (Raw assembly is still there if you
want it later: [writing-programs.md](writing-programs.md).)

## 1. Build the toolchain

You need a Rust toolchain (`rustup.rs`). Then, from the repository root:

```sh
cargo build
```

That builds everything: the CPU (`s16-core`), the assembler (`s16-asm`), the
compiler (`s16c`), and the emulator (`s16-emulator`).

## 2. Compile and run

The repository ships with
[programs/s16c/walk.s16c](../programs/s16c/walk.s16c), a small program that
moves an `@` around the screen with the arrow keys:

```sh
cargo run -p s16c -- programs/s16c/walk.s16c --bin
cargo run -p s16-emulator -- --tui programs/s16c/walk.bin
```

A green `@` appears on a 40×25 screen. Arrow keys move it; esc quits.

## 3. Read what happened

- **`--bin`** chains the compiler and the assembler in one step: `.s16c`
  source → readable `.s16` assembly → a flat binary image, the bytes that
  sit in the machine's memory starting at `0x0000`.
- **`--tui`** runs the full *machine* — display, keyboard, timers — in your
  terminal, not just the bare CPU. That's what lets you see the screen and
  press arrow keys; more on the CPU-alone "batch" mode in
  [writing-programs.md](writing-programs.md).
- The status bar shows the tick count and CPU state; esc quits cleanly.

`walk.s16c` itself is short — `use screen; use input; use time;`, then a
`loop` that reads the keys, erases the old position, redraws at the new one,
and paces itself with `wait_tick()`. No raw addresses anywhere; that's the
point of s16c's standard library (more in
[writing-s16c.md](writing-s16c.md)).

## 4. Change something

Open [programs/s16c/walk.s16c](../programs/s16c/walk.s16c) and change the
color:

```rust
put_cell(X, Y, '@', GREEN);
```

to `YELLOW` (or any of `BLACK GRAY RED GREEN YELLOW BLUE MAGENTA CYAN
WHITE`). Recompile and rerun — same two commands as step 2. That's the whole
loop: edit `.s16c`, `--bin`, run, look.

## 5. See the assembly it compiles to

Drop `--bin` and `s16c` writes readable `.s16` text instead of a binary
image:

```sh
cargo run -p s16c -- programs/s16c/walk.s16c    # writes programs/s16c/walk.s16
```

That output is ordinary S16 assembly — the same language as the hand-written
programs in `programs/asm/`, nothing about compiler output is special-cased.
Worth a skim once you're curious what `put_cell(...)` actually turns into.

Next: [writing-s16c.md](writing-s16c.md) for the full language tutorial —
types, control flow, functions, the standard library. If you'd rather work
one level down, in the assembly s16c itself compiles to,
[writing-programs.md](writing-programs.md) starts from scratch there
instead.
