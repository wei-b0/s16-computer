# S16 Architecture

S16 is a 16-bit computer implemented as a Solana program.

## Goals

- A complete programmable computer
- Simple, elegant architecture
- Educational and hackable

## Documents

| Document | Contents |
|----------|----------|
| [INSTRUCTION_SET.md](INSTRUCTION_SET.md) | Complete ISA: instructions, semantics, encodings, faults |
| [MEMORY_MAP.md](MEMORY_MAP.md) | 64 KB layout, MMIO devices, framebuffer, reset state |
| [CALLING_CONVENTION.md](CALLING_CONVENTION.md) | v1 software convention for calls and register use |
| [ASSEMBLY.md](ASSEMBLY.md) | Assembly syntax, directives, worked encoding example |
| [ASSEMBLER.md](ASSEMBLER.md) | The `s16-asm` contract: two-pass model, output format, CLI, error reporting |
| [EMULATOR.md](EMULATOR.md) | The `s16-emulator` contract: batch mode, interactive `--tui` machine, rendering, input |
| [LANGUAGE.md](LANGUAGE.md) | s16c — the tiny Rust-flavored game language: types, grammar, semantics, builtins |
| [COMPILER.md](COMPILER.md) | The `s16c` compiler contract: pipeline, CLI, emitted-code rules, acceptance vectors |
| [DESIGN_DECISIONS.md](DESIGN_DECISIONS.md) | The reasoning behind every architectural choice |

## CPU

| Property | Value |
|----------|-------|
| Word size | 16 bits |
| Registers | R0–R15, sixteen 16-bit general-purpose registers |
| Instruction width | 32 bits, fixed |
| ISA style | Minimal RISC, load/store |
| Endianness | Little-endian |

## CPU State

- R0–R15 — general-purpose registers
- PC — program counter (byte address of the current instruction; instructions are 4-byte aligned)
- SP — stack pointer (stack grows downward)
- FLAGS — condition flags, written only by `CMP`/`CMPI`:

| Bit | Flag | Meaning |
|-----|------|---------|
| 0 | Z | zero |
| 1 | N | negative (bit 15 of result) |
| 2 | C | carry/borrow (unsigned below) |
| 3 | V | signed overflow |

- Execution state — one of **Running**, **Halted** (`HALT`), **DebugHalt** (`BRK`), **Faulted** (fault code recorded)

## Memory

| Property | Value |
|----------|-------|
| Address space | 16-bit, byte-addressable |
| Size | 64 KB |
| Model | Unified mutable memory — code, data, and MMIO share one address space |
| Alignment | 16-bit accesses 2-byte aligned; instructions 4-byte aligned; violations fault |

Program and data share memory; MMIO regions (framebuffer, input, system devices) live at `0xF000–0xFFFF` and are accessed with ordinary loads and stores. See [MEMORY_MAP.md](MEMORY_MAP.md).

## Reset

PC = `0x0000`, SP = `0xF000`, all registers and FLAGS zero, state Running.

## Faults

No interrupts or exception handlers in v1. Illegal opcodes, misaligned accesses, and division by zero move the CPU to the Faulted state with a fault code, deterministically. See [INSTRUCTION_SET.md](INSTRUCTION_SET.md#faults).
