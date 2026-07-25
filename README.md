# S16

A 16-bit computer implemented as a Solana program — eventually running snake on-chain.

## Layout

| Path | Contents |
|------|----------|
| [specs/](specs/) | The architecture specification (source of truth): [ARCHITECTURE.md](specs/ARCHITECTURE.md), [INSTRUCTION_SET.md](specs/INSTRUCTION_SET.md), [MEMORY_MAP.md](specs/MEMORY_MAP.md), [CALLING_CONVENTION.md](specs/CALLING_CONVENTION.md), [ASSEMBLY.md](specs/ASSEMBLY.md), [DESIGN_DECISIONS.md](specs/DESIGN_DECISIONS.md) |
| [crates/s16-core](crates/s16-core/) | The CPU as a pure, deterministic, `no_std` library — shared by every host |
| [crates/s16-emulator](crates/s16-emulator/) | Local emulator binary (rendering, input, image loading) |
| [crates/s16-asm](crates/s16-asm/) | Assembler: `.s16` source → flat binary image |
| [programs/](programs/) | Example S16 programs: `sum`, `fib`, `calls`, `hello` — later snake |
| [docs/](docs/) | How-to guides: getting started, writing programs, debugging, embedding the CPU |

Planned: `s16-program` (the Solana program), `snake` (game source in S16 assembly).

## Building

```sh
cargo build            # everything
cargo test             # spec test vectors pin core and assembler to specs/

cargo run -p s16-asm -- programs/sum.s16      # assemble -> programs/sum.bin
cargo run -p s16-emulator -- programs/sum.bin # run it
```
