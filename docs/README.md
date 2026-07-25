# S16 Guides

How-to guides for using S16. For *what the machine is* — instructions,
encodings, memory map — see [specs/](../specs/); these pages are about *doing
things with it*.

| Guide | You'll learn |
|-------|--------------|
| [getting-started.md](getting-started.md) | Build the toolchain, assemble and run your first program, read the output |
| [writing-programs.md](writing-programs.md) | Write S16 assembly: loops, branches, data, subroutines |
| [debugging.md](debugging.md) | Use BRK as a breakpoint, decode faults, diagnose the classic mistakes |
| [using-s16-core.md](using-s16-core.md) | Embed the CPU in your own Rust code and drive it step by step |

Quick reference while writing code:

- Instruction semantics: [specs/INSTRUCTION_SET.md](../specs/INSTRUCTION_SET.md)
- Addresses and devices: [specs/MEMORY_MAP.md](../specs/MEMORY_MAP.md)
- Assembly syntax and directives: [specs/ASSEMBLY.md](../specs/ASSEMBLY.md)
- Register-use conventions: [specs/CALLING_CONVENTION.md](../specs/CALLING_CONVENTION.md)
