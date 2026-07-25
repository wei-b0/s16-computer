# Getting Started

Run your first S16 program in about two minutes.

## 1. Build the toolchain

You need a Rust toolchain (`rustup.rs`). Then, from the repository root:

```sh
cargo build
```

That builds everything: the CPU (`s16-core`), the assembler (`s16-asm`), and
the emulator (`s16-emulator`).

## 2. Assemble a program

The repository ships with [programs/sum.s16](../programs/sum.s16), which sums
five numbers:

```sh
cargo run -p s16-asm -- programs/sum.s16
```

```
programs/sum.s16 -> programs/sum.bin (54 bytes)
```

`sum.bin` is a flat memory image: the bytes that will sit in the machine's
memory starting at address `0x0000`. Use `-o other.bin` to pick the output
name.

## 3. Run it

```sh
cargo run -p s16-emulator -- programs/sum.bin
```

```
halted after 41 instructions
PC=0x002C  SP=0xF000  FLAGS=0b0001
R0 =0x0096  R1 =0x0036  R2 =0x0000  R3 =0x0032
R4 =0x0000  ...
```

## 4. Read the output

- **`halted after 41 instructions`** — the program reached `HALT` cleanly.
  You can also see `hit BRK` (a breakpoint), `FAULT ...` (the program did
  something illegal — see [debugging.md](debugging.md)), or `still running`
  (the safety budget of 100 M instructions ran out, i.e. probably an infinite
  loop).
- **`R0 =0x0096`** — `sum.s16` leaves its result in R0, and `0x0096` is 150 =
  10+20+30+40+50. Registers are always printed in hex.
- **`PC=0x002C`** — where execution stopped: just past the `HALT` at `0x0028`.
- The emulator's exit code is 0 for a clean stop (`HALT`/`BRK`) and 1 for
  anything else, so you can use it in scripts and CI.

## 5. Change something

Open [programs/sum.s16](../programs/sum.s16), change the data line:

```asm
data:   .word 100, 200, 300, 400, 500
```

Reassemble and rerun — R0 is now `0x05DC` (1500). That's the whole loop:
edit `.s16`, assemble, run, read registers.

Next: [writing-programs.md](writing-programs.md) to write your own program
from scratch.
