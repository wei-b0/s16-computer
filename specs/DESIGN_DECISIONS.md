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

## D023: Module-Qualified Standard Library Calls

Decision:
Every standard-library call is written `module::name` — `screen::put`, `debug::print_u16`, `rng::next` — layered on top of D022's `use module;` requirement, not a replacement for it. There is no bare-name fallback after `use`, and no `use module::item;` single-item import form; the only way to call a library item is its full qualified path.

Reason:
D022 made a program's *file* say what it depends on, via its `use` lines — but any individual *call site* still looked identical to a call to the program's own code. Rust's fully-qualified paths (`std::mem::swap(...)`, valid with no `use` at all) are the closer model than Rust's `use`-then-bare-name convenience: `screen::put(...)` names its own dependency on sight, without cross-referencing the file's `use` lines. This also turns the standard library into five genuinely independent namespaces instead of one shared flat one — two libraries can now define an internal helper with the same name (`write_byte` in both `screen` and `debug`) without colliding, something D020's single flat namespace could not allow.

Consequences:

- D020's whole-namespace shadowing rule is gone (see D022's note above) — impossible to trigger, not just discouraged, since a program can never write `::` in a name it defines
- `imports.rs` gained real per-library resolution logic (bare names inside a library body resolve within that library; qualified names from user code resolve against their named library); `check.rs` and `codegen.rs` needed no structural changes, since a qualified name is just an ordinary string that happens to contain `::`
- Every call site is self-documenting about which library it depends on, at the cost of the `module::` prefix on every single call — a program calling `put` fifty times writes `screen::put` fifty times, with no shorthand

## D024: String Literals Are Call-Argument-Only, Not a Type

Decision:
`"..."` is legal in exactly one place: as a direct argument to a parameter declared `str` — a marker legal only in function-parameter position (never for `var`/`static`/`const`/return types). `str` is not a general type: inside the function body the parameter is an ordinary `u16` (the literal's compile-time address, in the data section). Enforced at parse time, not just by the type checker — no other rule in the grammar ever accepts a string token.

Reason:
`debug::print("Hello")` and `screen::print(x, y, "Hello", color)` need *some* way to pass text, but a general string type would mean deciding what a string variable holds, how it's compared, whether it can grow — none of which this project wants (D015's no-heap stance, D014's word-sized-types-only model). A string literal is compile-time data plus an address, and nothing else needs to exist for that: no `String`, no runtime concatenation, no heap.

Consequences:

- No `var`/`static` can ever hold a string literal — `var x: u16 = "hi";` is a parse error, not a type error, since the grammar's only string-accepting production is a call argument
- `debug::print`/`println` and `screen::print` are the only v1 functions with a `str` parameter; a user *can* declare their own `fn f(s: str)` (nothing stops it, per D020's "library is ordinary s16c" precedent — `str` params aren't special-cased to standard-library code)
- A `str` parameter's *argument*, unlike the literal token itself, isn't restricted to a literal — an ordinary `u16` expression type-checks too (discovered necessary during implementation: `debug::println`'s own body forwards its `s` parameter to `debug::print(s)`, and `s` there is just a local variable, not a literal). Once a value has entered a program as a `str` parameter, passing it onward is an ordinary value pass, with the same "no bounds checking" caveat as array indexing — nothing stops passing an unrelated `u16` where a string address is expected
- Codegen emits string bytes as `.byte` (not `.asciiz`) specifically because the assembler's string-escaping only covers ASCII plus seven named escapes and can't round-trip arbitrary byte values — `.byte`'s numeric form has no such gap

## D025: A Debug-Console MMIO Device, Drained Per Tick

Decision:
A 256-byte buffer at `0xF800` (`DEBUG_BUF`) plus a length register at `0xF900` (`DEBUG_LEN`) — the program writes text via `poke8`, incrementing `DEBUG_LEN` as it goes; the host reads it once per tick and resets `DEBUG_LEN` to `0` (`Machine::tick`, matching D011's per-tick device-servicing model). Batch mode, which doesn't tick, gets one narrow exception: a single final read after the CPU stops, so a non-interactive program can still print a result.

Reason:
Games and simple examples alike had no way to get a bare computed value out of a program short of `poke`ing it to a fixed address and reading memory directly from Rust (or drawing it to the framebuffer, if the program already touched the screen for something else). Every other device on this map is host-writes/CPU-reads (INPUT, RNG, TICK, STATUS); a debug console needed to go the other direction, and the memory-mapped, once-per-tick model already established for every other device (D007, D011) extends cleanly to that direction too — the framebuffer is a close analogy, just written by the CPU and sampled by the host instead of the reverse.

Consequences:

- The memory map's first CPU-writes/host-reads register — everything else on the page is the other way around
- Host-agnostic by construction: the local emulator prints/shows it (batch: one-shot final read; `--tui`: the status bar, replacing tick/state text for the tick that produced it); a future Solana host could map the same bytes to program logs, or ignore them entirely, with no change to `debug.s16c` or the device itself
- A single tick's (or, in batch mode, a whole run's) debug output is capped at 256 bytes — writes past capacity are silently dropped (`debug.s16c`'s `write_byte` checks `DEBUG_LEN` before writing), the same class of documented sharp edge as unchecked array indexing (LANGUAGE.md §6)
- No scrollback in `--tui` mode in v1 — the status bar always shows only the most recent tick's debug text

*(Note, appended alongside D023: library calls are now module-qualified
(`screen::put`, not bare `put`), so the shadowing behavior described above
and in D020 no longer applies — a program's own names and a library's
qualified names can't collide in the first place, so there's nothing left
to shadow. `use` itself is unchanged: still required, still gates
visibility exactly as described here.)*

## D026: WAIT / Waiting, Woken by Input

Decision:
A zero-operand `WAIT` instruction (opcode `0x03`) puts the CPU into a new `Waiting` state. Entering `Waiting` stops execution immediately — `Cpu::run()`'s loop already exits the instant `state != Running`, so no separate "budget" logic was needed to make this true. `Waiting` does not auto-resume at the next tick: `Machine::tick()` wakes it back to `Running` only when it observes a nonzero `INPUT` bitmask that tick (a keypress) — otherwise the CPU stays parked and the tick executes zero instructions. Batch mode never writes `INPUT` at all, so a `WAIT`ed program there simply never wakes; that is treated as the correct terminal outcome for a mode with no external event source, not a bug to work around.

Reason:
Before this, the only way for a program to be "idle" was to busy-loop, spending its full per-tick budget every tick even when genuinely waiting for a keypress — wasteful, and it made `Machine::tick()`'s existing "instructions actually executed" return value uninteresting (it was always either the budget or a HALT/fault point). `WAIT` gives a program an explicit way to say "nothing to do until something external happens," which both stops wasted execution immediately and mirrors the model the eventual on-chain host needs: a transaction that finds the CPU parked can skip execution entirely instead of paying gas for a busy-loop. Wake-on-`INPUT` (rather than auto-resuming every tick, or building a general interrupt controller) was chosen because `INPUT` is the only genuine external-event source the machine has today — inventing a fuller interrupt system ahead of any concrete need would be speculative.

Consequences:

- `status_word()` gains a fourth non-zero state value (`4`); STATUS's low byte is no longer just Running/Halted/DebugHalt/Faulted
- The emulator's instructions-per-second figures (new in both batch mode and the `--tui` status bar) are the first place the emulator reports an actual execution rate rather than a raw instruction count — meaningful specifically because `Waiting` can now make that rate drop below `budget * tick_rate`
- A `Waiting` CPU in `--tui` mode needs no dedicated resume key, unlike `DebugHalt`'s `r`: it wakes through the same key-press path that already produces the `INPUT` mask fed to `Machine::tick()` every frame
- If a real interrupt/event system is ever added, it would extend the wake condition (more event sources waking a `Waiting` CPU), not replace this mechanism

## D027: `wait()` and `input::wait_key()` — Exposing WAIT to s16c

Decision:
A raw `wait()` builtin (compiles straight to `WAIT`) joins `peek`/`poke`/`peek8`/`poke8`/`halt`/`brk` in LANGUAGE.md §7.6 — no `use` needed, one instruction, unlike `halt()` it *does* return once the host wakes the CPU. On top of it, `input::wait_key() -> u16` (`crates/s16c/src/libs/input.s16c`) is the ergonomic library wrapper: read the current keys, and if none are held, `wait()` then re-read, so it always returns a nonzero bitmask. The two are given distinct names (`wait` vs `wait_key`), not the same name at two tiers the way `screen::put`/raw-`poke` conceptually pair up.

Reason:
D026 put `WAIT` in the CPU and machine layer, but nothing in s16c could reach it — the project's running goal (D023) is for the standard library to read like a small subset of Rust, not to leave hardware capability behind a raw opcode wrapper only reachable by writing MMIO addresses by hand. `walk.s16c` is the concrete motivating case: it busy-spins every tick (via `time::wait_next()`'s tick-polling loop, which can't return until `TICK` itself changes, so it burns the rest of that tick's budget even when idle) whether or not a key is held; `input::wait_key()` lets it park instead.

The distinct-name requirement is not a style preference — it's forced by how library qualification resolves bare names (D023, `imports.rs`'s `maybe_qualify`): a bare call inside a library's own body is checked against *that library first*. Had the wrapper also been named `wait`, the bare `wait()` call inside `input.s16c`'s own `wait_key()` body — meant to reach the raw CPU builtin — would instead have been qualified to `input::wait` (a self-reference), producing infinite recursion that's indistinguishable at the syntax level from genuine self-recursion.

Consequences:

- Any future library wrapper around a bare builtin must pick a distinct name from that builtin, for the same reason — this is now a standing rule, not a one-off
- `walk.s16c` gained one new line (drawing the start position once before the loop) so the visible '@' still appears immediately with no input held, since `wait_key()` now parks at the top of the loop before that iteration's erase/redraw
- `crates/s16c/tests/vectors.rs`'s `v4_devices_walk_program` now asserts `State::Waiting` (not `State::Running`) once no key is held — the loop legitimately parks between keypresses instead of eternally busy-spinning, the intended effect of this change, not a regression

## D028: WAITTICK / WaitingTick — Fixing `wait_next()`'s Busy-Poll

Decision:
A second zero-operand instruction, `WAITTICK` (opcode `0x04`), puts the CPU into a new `WaitingTick` state — parallel to D026's `WAIT`/`Waiting`, but woken unconditionally on the next `Machine::tick()` call rather than only when `INPUT` is nonzero. Because `TICK` is written once per tick before the CPU runs (Machine Tick step 1) and is then fixed for that whole tick's CPU slice, "wake on tick advance" and "wake on the very next tick, always" are the same condition — there's no "stays parked" case to speak of the way there is for `WAIT`. `time::wait_next()` (`crates/s16c/src/libs/time.s16c`) is rewritten from a busy-poll loop (`while peek(0xFF04) == t {}`) to a single call to the new `wait_tick()` builtin, which compiles straight to `WAITTICK`, following the same builtin-plus-library-wrapper shape as D027's `wait()`/`wait_key()`.

Reason:
D027 named this bug without fixing it: `wait_next()`'s poll loop reads `TICK`, which cannot change until the *next* tick's device-write phase — a phase that only runs after the current tick's CPU slice has already ended. So the loop can never observe a change and exit within the tick it started in; it always spins for the entire remainder of that tick's instruction budget, unconditionally, regardless of how little real work the program does. Any program pacing itself with `wait_next()` — which is every s16c game loop, since it's the standard frame pacer — therefore reports "100% budget consumed" every tick even when idle, and the budget has to be sized to whatever the host's tick rate demands rather than to the program's actual work. That's cosmetically odd in the local emulator but a real problem for the eventual Solana host (D026's motivating case): compute units would be spent on pure no-op spinning instead of gameplay logic, on every single transaction. Extending the `Waiting` mechanism with a second, tick-driven wake condition — rather than making `WAIT` itself wake on either input or tick — keeps `wait_key()`'s existing "genuinely idle costs zero instructions across many ticks" property intact; folding the two together would make every `wait_key()` wake up and re-check on every tick even with no key held, trading a cheap park for a cheap-but-nonzero one for no benefit.

Consequences:

- `status_word()` gains a fifth non-zero state value (`5`) for `WaitingTick`; STATUS's low byte is now Running/Halted/DebugHalt/Faulted/Waiting/WaitingTick
- `Machine::tick()`'s wake step now has two independent clauses — `Waiting` wakes on nonzero `INPUT`, `WaitingTick` wakes unconditionally — rather than one; this is the "extend the wake condition" path D026 anticipated
- Batch mode (D025, never ticks) can never wake a `WAITTICK`ed program either, for the same reason a `WAIT`ed one never wakes there: no external event source and, for `WAITTICK` specifically, no notion of "next tick" at all
- `spin.s16c` and `walk.s16c` (and any program using `time::wait_next()`) now report real per-tick instruction counts reflecting their actual work (tens of instructions) instead of always equaling the budget — this is the intended effect, not a regression, and is the property the eventual Solana host needs to meter compute honestly
- `crates/s16c/tests/libraries.rs`'s `wait_next_blocks_for_exactly_one_tick` now asserts `State::WaitingTick` after the call (not `State::Running`), and that the poked value isn't written until the following tick — the loop legitimately parks instead of spinning out the budget
