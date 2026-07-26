# S16 Memory Map

S16 has a single, unified, byte-addressable 64 KB memory. From the CPU's perspective there is only one kind of operation: a memory access. MMIO regions are ordinary addresses that happen to have device semantics attached.

All multi-byte values are **little-endian**.

## Layout

| Range | Size | Region |
|-------|------|--------|
| `0x0000–0xEFFF` | 60 KB | General-purpose memory: code, data, stack |
| `0xF000–0xF7FF` | 2 KB | MMIO: framebuffer |
| `0xF800–0xF8FF` | 256 B | MMIO: debug-console output buffer |
| `0xF900–0xF901` | 2 B | MMIO: debug-console output length |
| `0xF902–0xFEFF` | ~1.75 KB | MMIO: reserved for future devices |
| `0xFF00–0xFFFF` | 256 B | MMIO: system devices |

## General-Purpose Memory (`0x0000–0xEFFF`)

Code, data, and the stack share this region freely — programs and data are not segregated (see D001). Execution begins at `0x0000`. The stack starts at `0xF000` and grows downward into this region (the first `PUSH` writes to `0xEFFE`).

There is no memory protection: a program may read, write, and execute any address, including its own code.

## Framebuffer (`0xF000–0xF7FF`)

A 40×25 character grid, 2 bytes per cell, row-major:

```
cell(row, col) at 0xF000 + (row * 40 + col) * 2     row ∈ [0,24], col ∈ [0,39]
```

| Offset in cell | Byte | Meaning |
|----------------|------|---------|
| +0 | character | glyph code (rendering/charset defined by the display layer) |
| +1 | attribute | bits 0–3: foreground color, bits 4–6: background color, bit 7: blink |

- 1000 cells × 2 bytes = 2000 bytes: `0xF000–0xF7CF`. The tail `0xF7D0–0xF7FF` is unused padding.
- The 16-color palette and glyph set are defined by the renderer (off-chain), not the CPU. *(v1 stub — to be specified in the display/renderer phase.)*
- Writes take effect immediately; the renderer samples the framebuffer whenever it draws a frame.

Writing a cell as a single 16-bit `STW` (attribute in the high byte, character in the low byte) is the idiomatic fast path.

## Debug Console (`0xF800–0xF901`)

Text output for `debug::print`/`println`/`print_u16`/`print_i16`/`print_hex`
(s16c `debug` library) — *not* the framebuffer, and host-agnostic (D025):
the local emulator prints it, a future Solana host could map it to program
logs or ignore it entirely.

| Address | Name | Access | Semantics |
|---------|------|--------|-----------|
| `0xF800–0xF8FF` | `DEBUG_BUF` | W | 256-byte buffer the program writes text into, one byte at a time (`poke8`). |
| `0xF900` | `DEBUG_LEN` | RW | How many bytes of `DEBUG_BUF` are valid. The program increments this as it writes; the host resets it to `0` once it has drained the buffer, each tick (below). |

This is the memory map's first **CPU-writes/host-reads** register — every
other device on this page is host-writes/CPU-reads. Writing past byte 255
without a drain in between is silently dropped (`debug.s16c`'s `write_byte`
checks `DEBUG_LEN` before writing) — a known, documented v1 limit, the same
class of sharp edge as unchecked array indexing in LANGUAGE.md.

## Reserved (`0xF902–0xFEFF`)

Reserved for future devices (e.g., audio, storage, network). Reads return 0; writes are ignored. Do not use.

## System Devices (`0xFF00–0xFFFF`)

All device registers are 16-bit and 2-byte aligned.

| Address | Name | Access | Semantics |
|---------|------|--------|-----------|
| `0xFF00` | `INPUT` | R | Key state bitmask: bit 0 = Up, 1 = Down, 2 = Left, 3 = Right, 4 = A, 5 = B, 6 = Start, 7 = Select. 1 = held. Refreshed once per machine tick. |
| `0xFF02` | `RNG` | R | 16-bit pseudorandom value, advanced **once per machine tick** (all reads within a tick see the same value). Generator: xorshift16 — `x ^= x<<7; x ^= x>>9; x ^= x<<8` — state never zero; seed chosen by the host at power-on (seed 0 is treated as 1). |
| `0xFF04` | `TICK` | R | Machine tick counter: 0 on the first tick, +1 per tick, wraps mod 0x10000. |
| `0xFF06` | `STATUS` | R | Execution status, written by the machine after each tick's CPU slice: low byte = state (0 Running, 1 Halted, 2 DebugHalt, 3 Faulted, 4 Waiting, 5 WaitingTick), high byte = fault code. |
| `0xFF08–0xFFFF` | — | — | Reserved. Reads return 0; writes ignored. |

## Machine Tick

The machine runs the CPU in **ticks** (implemented by `s16-machine`). Each tick:

1. The machine writes the device registers: `INPUT` (host-provided key state), `RNG` (advanced once), `TICK` (current count).
2. The machine wakes a parked CPU, if any: **Waiting** (D026, `WAIT` instruction) wakes back to Running only if this tick's `INPUT` is nonzero — a keypress is the only external input event the machine has today. **WaitingTick** (D028, `WAITTICK` instruction) wakes back to Running unconditionally, every tick, since `TICK` has just advanced in step 1 — there is no analogous "stay parked" case for it. A `Waiting` CPU that isn't woken this way stays parked, executes nothing, and the tick returns `0` instructions executed.
3. The CPU executes up to a host-chosen **budget** of instructions (or until it stops). Reaching `Halted`, `DebugHalt`, `Faulted`, `Waiting`, or `WaitingTick` always stops execution before the budget is exhausted — the number of instructions actually executed this tick can be anywhere from `0` up to `budget`, and hosts should treat that number (not the budget) as the true cost/rate of the tick.
4. The machine reads `DEBUG_LEN`/`DEBUG_BUF`, exposes that tick's debug text to the host (replacing, not accumulating, whatever the previous tick exposed), and resets `DEBUG_LEN` to `0` so the program can keep appending next tick.
5. The machine writes `STATUS` and increments the tick counter.

Hosts map ticks to their own time base — the emulator runs one tick per rendered frame; the Solana host runs one (or more) per transaction. Device registers keep updating after the CPU stops, so renderers stay live. Determinism rule: given the same seed, per-tick inputs, and budgets, every host produces the identical machine state.

A `Waiting` or `WaitingTick` CPU is specifically useful to a host like the eventual Solana one: a transaction that finds the CPU parked can skip execution entirely instead of paying for a busy-loop — whether that loop was spinning until some input arrived, or just spinning out the rest of a tick waiting to be paced by the next one.

Batch mode (no ticking at all — see [EMULATOR.md](EMULATOR.md)) does not
service any device this way; it gets one narrow exception, reading
`DEBUG_LEN`/`DEBUG_BUF` a single time after the CPU stops, so a simple
non-interactive program can still print a result. Since batch mode never ticks at all, neither a `WAIT` nor a `WAITTICK` there
can ever be woken — the CPU simply stays parked forever, which is the
correct (if unexciting) outcome for a mode that has no external event
source, and no notion of "next tick," at all.

How key events become the `INPUT` bitmask (keyboard in the emulator, transaction data on-chain) remains a host concern.

## Reset State

| State | Value |
|-------|-------|
| PC | `0x0000` |
| SP | `0xF000` |
| R0–R15 | `0x0000` |
| FLAGS | all clear |
| Execution state | Running |
| Memory | as loaded (program image); unspecified bytes are `0x00` |

## Snake Sufficiency Walkthrough

A sanity check that this memory map plus the [instruction set](INSTRUCTION_SET.md) is sufficient for the end-goal snake game. Core game loop, on paper:

```
loop:
  read INPUT (LDW from 0xFF00), update direction
  compute next head cell from (row, col) + direction
  read that framebuffer cell (LDW): if body glyph -> game over (HALT)
  if food glyph -> grow, read RNG (LDW from 0xFF02) mod 1000 to place new food
  write head cell (STW glyph+attr), erase tail cell (STW blank)
  wait for next tick (WAITTICK — parks the CPU rather than polling TICK; D028)
  JMP loop
```

Representative fragments:

```asm
; read input, test "Right" (bit 3)
LDI  R1, #0xFF00
LDW  R2, [R1+0]        ; R2 = key bitmask
ANDI R2, R2, #0x0008
CMPI R2, #0
BNE  turn_right

; write snake-head glyph '#' with attribute 0x02 to cell (row=R4, col=R5)
LDI  R1, #40
MUL  R3, R4, R1        ; row * 40
ADD  R3, R3, R5        ; + col
SHLI R3, R3, #1        ; * 2 bytes/cell
ADDI R3, R3, #0xF000   ; + framebuffer base
LDI  R2, #0x0223       ; attr 0x02, char 0x23 ('#')
STW  [R3+0], R2
```

Everything the game needs — input, deterministic randomness, timing, collision detection via framebuffer read-back, and drawing — is reachable through plain loads and stores. No ISA or memory-map extensions required.
