# S16 Design Decisions

## D001: Unified Mutable Memory

Decision:
Programs and data share the same memory.

Reason:
S16 should behave like a normal computer.

Consequences:

- Self-modifying code possible
- Simple architecture
- No separate ROM/RAM model

## D002: Single Account Memory

Decision:
S16 v1 stores CPU state and memory together.

Reason:
Prioritize simplicity.

Consequences:

- Easier implementation
- Less scalable
- Future paging possible

## D003: Public and Private Computers

Decision:
Support both.

Reason:
Different use cases require different execution models.

## D004: Minimal RISC Load/Store ISA, Fixed 32-bit Encoding

Decision:
~40 instructions; ALU operates register-to-register; only loads/stores touch memory; every instruction is one 32-bit word.

Reason:
Trivial decode, cheap per-instruction cost on Solana compute, and the 32-bit width fits a full 16-bit immediate in one instruction (no LUI/split-immediate dance). MUL/DIV/MOD are included because they cost almost nothing in a software emulator.

Consequences:

- Simple emulator core (fetch 32-bit word, switch on 8-bit opcode)
- Larger code size than a 16-bit encoding (acceptable in 64 KB)
- Full instruction list in INSTRUCTION_SET.md

## D005: FLAGS-Based Compare-and-Branch

Decision:
Only CMP/CMPI write FLAGS (Z, N, C, V); branches test flags. No other instruction touches FLAGS.

Reason:
FLAGS was already part of CPU state, branch encodings stay small, and restricting flag writes to two instructions keeps the spec and emulator surface minimal.

Consequences:

- One CMP can feed several branches
- Arithmetic overflow detection requires an explicit CMP idiom (fine for v1)

## D006: Little-Endian, Byte-Addressable, Aligned Access

Decision:
Byte-addressable 64 KB, little-endian, 16-bit accesses 2-byte aligned, instructions 4-byte aligned; misalignment faults.

Reason:
Matches host CPUs and Solana data conventions; alignment faults catch bugs early and keep the emulator branch-free on the fast path.

## D007: MMIO Reserved at 0xF000+ Within Unified Memory

Decision:
Top 4 KB of the address space is reserved for MMIO now: framebuffer at 0xF000, system devices at 0xFF00.

Reason:
Snake will need a display and input eventually; reserving addresses now avoids a breaking memory-map change later. From the CPU's view MMIO is a plain memory access — unified memory (D001) is preserved.

Consequences:

- 60 KB general-purpose memory
- Device registers are stubs where later phases must decide semantics (RNG algorithm, tick source)

## D008: 40×25 Character-Grid Display, 2 Bytes per Cell

Decision:
The display is a 40×25 grid of cells; each cell is a character byte plus an attribute byte (color, blink, future extensions).

Reason:
~2 KB framebuffer fits comfortably in a Solana account, renders trivially off-chain, and is plenty for snake. The attribute byte buys color and future features for one extra byte per cell.

## D009: Fault-Halts, No Interrupts

Decision:
Illegal opcode, misaligned access, and divide-by-zero move the CPU to a Faulted state with a fault code. Execution states: Running, Halted, DebugHalt (BRK), Faulted.

Reason:
Determinism and simplicity — essential on-chain, and BRK gives the emulator/debugger a first-class breakpoint.

Consequences:

- No interrupt controller, no handler tables
- Games poll INPUT/TICK instead of receiving events

## D010: PC-Relative Control Flow

Decision:
JMP, CALL, and all branches use a signed 16-bit instruction offset (target = PC + 4 + offset×4). Only JR is absolute.

Reason:
Relocatable, position-independent code and natural label handling in the assembler. The ±32K-instruction range covers the entire address space.

Consequences:

- Code can be loaded at any 4-byte-aligned address
- Computed jumps/jump tables go through JR

## D011: Tick-Based Machine Loop

Decision:
The machine runs in ticks: service device registers, execute a budget of CPU instructions, update STATUS. The CPU itself has no notion of time or devices.

Reason:
One loop shape fits every host — emulator frame, Solana transaction — and keeps the CPU core pure. Polling devices once per tick is deterministic; interrupt-style delivery would not be (see D009).

Consequences:

- Programs poll INPUT/TICK; a game's speed derives from TICK, not instruction count
- The instruction budget is the compute knob for on-chain execution
- Device semantics live in s16-machine, shared by all hosts

## D012: Per-Tick xorshift16 RNG

Decision:
RNG holds a value advanced once per machine tick by a 16-bit xorshift (`x^=x<<7; x^=x>>9; x^=x<<8`), seeded by the host; state never zero.

Reason:
"New value per read" cannot be expressed as plain memory (D007) — per-tick update can, and it is trivially reproducible on-chain. Xorshift is a few shifts and xors: cheap everywhere, full 65535 period, plenty for games.

Consequences:

- All RNG reads within one tick see the same value (mix with TICK or program state if more entropy is needed per tick)
- Replays are exact given seed + inputs + budgets

## D013: s16c — A Tiny Rust-Flavored Language

Decision:
Games get a native language, s16c: Rust-like surface (fn/let/if/while/loop), compiled by our own compiler to S16 assembly text, with s16-asm as the backend.

Reason:
Real rustc cannot target a 16-bit machine (LLVM/Cranelift have no 16-bit pointer support), and raw assembly doesn't scale to games. Emitting assembly text keeps the compiler simple, makes its output debuggable, and reuses the assembler we already trust.

Consequences:

- LANGUAGE.md and COMPILER.md join the spec set; CALLING_CONVENTION.md becomes a binding ABI
- Compiler correctness is pinned behaviorally (acceptance vectors), not byte-for-byte

## D014: Word-Sized Types — u16, i16, bool

Decision:
Three compile-time types, all one 16-bit word at runtime. No implicit conversions; `as` casts reinterpret bits.

Reason:
The type checker's one hard job on this CPU is signedness — picking BLT vs BLTU, SAR vs SHR. Two int types plus bool buys that with minimal compiler complexity.

## D015: Globals and Fixed Arrays, No Heap

Decision:
Data lives in `static` words and fixed-size static arrays, plus stack locals. No heap, no structs in v1 (parallel arrays instead).

Reason:
64 KB and game workloads don't need an allocator; statics compile to `.word` data in the image. Structs are pure compiler surface with no new runtime concept — a clean v2 addition once snake ships.

## D016: MMIO via peek/poke Builtins

Decision:
`peek`/`poke`/`peek8`/`poke8` builtins (plus `halt`, `brk`) are the language's whole I/O surface; there are no pointer types.

Reason:
Devices are already plain memory (D007). Builtins compile to single LDW/STW/LDB/STB instructions and keep pointers — the biggest source of language complexity — out of v1 entirely.

## D017: ISA v1.1 — RSP/WSP, and FP-Based Frames

Decision:
Two new instructions, `RSP rd` (rd = SP) and `WSP rs1` (SP = rs1, alignment-checked), and an ABI with R15 as frame pointer and standard prologue/epilogue.

Reason:
v1.0 exposed SP only implicitly (PUSH/POP/CALL/RET), so functions could not address stack locals — the compiler made this gap concrete. With frames, recursion works by construction.

Consequences:

- Opcode count 43 → 45; core, assembler, and tests updated in lockstep
- R15 is no longer a general temporary in ABI-conforming code

## D018: Division Is Unsigned-Only in v1

Decision:
s16c permits `/` and `%` on u16 only; on i16 they are compile errors.

Reason:
The CPU divides unsigned (D004 kept the ALU minimal). Signed division needs a software routine — worth adding as a library function when something actually needs it, not as hidden magic codegen in v1.

## D019: A Standard Library, Not Raw MMIO, as the Primary API

Decision:
Application code targets a platform API — `put_cell`, `read_input`, `wait_tick`, `random`, and friends (LANGUAGE.md §7) — not `peek`/`poke` on hardware addresses directly. s16c is a tiny game language with low-level hardware access available, not a low-level language that happens to support games.

Reason:
The language's grammar was always small; the developer-experience problem turned out to live one layer up. The first worked example (walk.s16c) required knowing the framebuffer base address, the cell stride, the attribute-byte layout, and the INPUT bitmask before writing a single line of game logic — correct systems programming, but the wrong default for "make Pong." `peek`/`poke` stay exactly as they are (D016) as the advanced escape hatch; they're just no longer the first thing a program reaches for.

Consequences:

- LANGUAGE.md §7 documents the library surface as part of the language, alongside the grammar
- The worked example and snake sketch (§9, §10) are rewritten to use it
- No language or ABI changes — this is purely an API-surface decision

## D020: The Standard Library Is s16c, Merged by Reachability

Decision:
The standard library is ordinary s16c source (`crates/s16c/src/prelude.s16c`), compiled into every program. Only library items the program actually references (transitively) are included; a user definition with the same name shadows the library's entirely, for every caller.

Reason:
Writing the library in s16c itself (rather than baking it into the compiler as special-cased codegen) means it is tested, read, and extended exactly like application code — and proves the language is expressive enough to build its own standard library, not just consume one. Reachability-based inclusion keeps unused library code out of the 64 KB image; whole-namespace shadowing means "I don't like how `clear_screen` works" is just writing a function, not forking the compiler.

Consequences:

- COMPILER.md's pipeline gains a merge step between parse and typecheck
- The merge must be deterministic (fixed inclusion order) — same source, same output, matching every other determinism guarantee in this project
- A bug in the library surfaces as a compiler bug, not a user error — it never should reach an end user

*(Superseded in part by D022: the library is no longer one file "compiled
into every program" — it's four files, each visible only if `use`d. The
reachability-based inclusion and shadowing rules described above are
unchanged; only what feeds them changed, from "everything" to "everything
the program `use`d.")*

## D021: `var`, Not `let`, for Mutable Locals

Decision:
Local variables are declared with `var`, not `let`.

Reason:
s16c locals are always mutable — there is no immutable binding and therefore no `mut` to distinguish (D014's word-sized-types simplicity extends here). Keeping Rust's `let` would silently promise Rust's default-immutable semantics to anyone who knows Rust; `var` is unambiguous on sight and matches what the language actually does, at the cost of one point of surface divergence from Rust.

## D022: Explicit `use` Imports Replace the Auto-Merged Prelude

Decision:
The standard library splits into four named libraries — `screen`, `input`, `time`, `random` — and a program must write `use screen;` (etc.) before any of that library's functions, statics, or constants become visible. Nothing is available by default beyond the six always-on hardware builtins (D016).

Reason:
D020's single auto-merged prelude made every program's dependencies invisible in its own source — nothing said "this program touches the screen," the compiler just noticed and included it. That's a step too implicit, closer to Rust's automatic prelude than to the explicit, opt-in model of C's `#include` this project wants: a reader (or the compiler, when a name is unresolved) should be able to tell exactly what a program depends on by looking at its top few lines.

Consequences:

- D020's reachability + shadowing *mechanism* is unchanged, just re-scoped: it now runs over the union of `use`d libraries instead of one fixed file (see D020's note)
- Every non-trivial program now opens with a handful of `use` lines — a small, permanent bit of ceremony traded for visible dependencies
- A call to a real library function whose library wasn't `use`d gets a specific "add `use X;`" diagnostic rather than a bare unknown-name error, so the fix is discoverable without consulting the library reference
- Still no package manager: the four libraries are a fixed, compiler-shipped set; `use` names a library, not a file path
