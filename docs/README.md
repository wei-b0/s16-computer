# S16 Guides

How-to guides for using S16. For *what the machine is* — instructions,
encodings, memory map — see [specs/](../specs/); these pages are about *doing
things with it*.

| Guide | You'll learn |
|-------|--------------|
| [getting-started.md](getting-started.md) | Build the toolchain, assemble and run your first program, read the output |
| [writing-programs.md](writing-programs.md) | Write S16 assembly: loops, branches, data, subroutines |
| [writing-s16c.md](writing-s16c.md) | Write games in s16c: the standard library (screen/input/time), types, control flow, functions |
| [debugging.md](debugging.md) | Use BRK as a breakpoint, decode faults, diagnose the classic mistakes |
| [using-s16-core.md](using-s16-core.md) | Embed the CPU in your own Rust code and drive it step by step |

Quick reference while writing code:

- Instruction semantics: [specs/INSTRUCTION_SET.md](../specs/INSTRUCTION_SET.md)
- Addresses and devices: [specs/MEMORY_MAP.md](../specs/MEMORY_MAP.md)
- Assembly syntax and directives: [specs/ASSEMBLY.md](../specs/ASSEMBLY.md)
- Register-use conventions: [specs/CALLING_CONVENTION.md](../specs/CALLING_CONVENTION.md)
- Assembler contract (CLI, errors, output format): [specs/ASSEMBLER.md](../specs/ASSEMBLER.md)
- Emulator contract (CLI, batch vs. `--tui`, rendering, input): [specs/EMULATOR.md](../specs/EMULATOR.md)
- The s16c language (types, grammar, builtins): [specs/LANGUAGE.md](../specs/LANGUAGE.md)
- The s16c compiler contract (CLI, codegen rules, vectors): [specs/COMPILER.md](../specs/COMPILER.md)

New to s16c? Start with [writing-s16c.md](writing-s16c.md) — or jump straight
in: `cargo run -p s16c -- programs/walk.s16c --bin`, then
`cargo run -p s16-emulator -- --tui programs/walk.bin` — arrow keys move the
`@`. `s16c file.s16c` alone emits readable `.s16` assembly instead.
