# S16

A 16-bit computer implemented as a Solana program — eventually running snake on-chain.

## Layout

| Path | Contents |
|------|----------|
| [specs/](specs/) | The architecture specification (source of truth): [ARCHITECTURE.md](specs/ARCHITECTURE.md), [INSTRUCTION_SET.md](specs/INSTRUCTION_SET.md), [MEMORY_MAP.md](specs/MEMORY_MAP.md), [CALLING_CONVENTION.md](specs/CALLING_CONVENTION.md), [ASSEMBLY.md](specs/ASSEMBLY.md), [DESIGN_DECISIONS.md](specs/DESIGN_DECISIONS.md) |
| [crates/s16-core](crates/s16-core/) | The CPU as a pure, deterministic, `no_std` library — shared by every host |
| [crates/s16-machine](crates/s16-machine/) | The machine: tick loop + MMIO devices (input, RNG, tick) — `no_std`, host-agnostic |
| [crates/s16-emulator](crates/s16-emulator/) | Local emulator: batch runs, or `--tui` for display + keyboard in the terminal |
| [crates/s16-asm](crates/s16-asm/) | Assembler: `.s16` source → flat binary image |
| [crates/s16c](crates/s16c/) | Compiler for [s16c](specs/LANGUAGE.md), the tiny Rust-flavored game language |
| [programs/](programs/) | Example programs: assembly (`sum`, `fib`, `calls`, `hello`, `spin`) and s16c (`walk`) |
| [docs/](docs/) | How-to guides: getting started, writing programs, debugging, embedding the CPU |

Planned: `s16-program` (the Solana program), `snake` (in s16c).

## Building

```sh
cargo build            # everything
cargo test             # spec test vectors pin core, assembler, machine, and compiler to specs/

cargo run -p s16-asm -- programs/sum.s16             # assemble -> programs/sum.bin
cargo run -p s16-emulator -- programs/sum.bin        # batch: run CPU, print registers
cargo run -p s16-emulator -- --tui programs/spin.bin # interactive machine (esc quits)

cargo run -p s16c -- programs/walk.s16c --bin        # compile s16c -> programs/walk.bin
cargo run -p s16-emulator -- --tui programs/walk.bin # play it (arrow keys move the @)
```
