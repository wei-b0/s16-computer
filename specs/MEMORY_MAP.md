# S16 Memory Map

S16 has a single, unified, byte-addressable 64 KB memory. From the CPU's perspective there is only one kind of operation: a memory access. MMIO regions are ordinary addresses that happen to have device semantics attached.

All multi-byte values are **little-endian**.

## Layout

| Range | Size | Region |
|-------|------|--------|
| `0x0000–0xEFFF` | 60 KB | General-purpose memory: code, data, stack |
| `0xF000–0xF7FF` | 2 KB | MMIO: framebuffer |
| `0xF800–0xFEFF` | ~1.75 KB | MMIO: reserved for future devices |
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

## Reserved (`0xF800–0xFEFF`)

Reserved for future devices (e.g., audio, storage, network). Reads return 0; writes are ignored. Do not use.

## System Devices (`0xFF00–0xFFFF`)

All device registers are 16-bit and 2-byte aligned.

| Address | Name | Access | Semantics |
|---------|------|--------|-----------|
| `0xFF00` | `INPUT` | R | Key state bitmask: bit 0 = Up, 1 = Down, 2 = Left, 3 = Right, 4 = A, 5 = B, 6 = Start, 7 = Select. 1 = held. Refreshed once per machine tick. |
| `0xFF02` | `RNG` | R | 16-bit pseudorandom value, advanced **once per machine tick** (all reads within a tick see the same value). Generator: xorshift16 — `x ^= x<<7; x ^= x>>9; x ^= x<<8` — state never zero; seed chosen by the host at power-on (seed 0 is treated as 1). |
| `0xFF04` | `TICK` | R | Machine tick counter: 0 on the first tick, +1 per tick, wraps mod 0x10000. |
| `0xFF06` | `STATUS` | R | Execution status, written by the machine after each tick's CPU slice: low byte = state (0 Running, 1 Halted, 2 DebugHalt, 3 Faulted), high byte = fault code. |
| `0xFF08–0xFFFF` | — | — | Reserved. Reads return 0; writes ignored. |

## Machine Tick

The machine runs the CPU in **ticks** (implemented by `s16-machine`). Each tick:

1. The machine writes the device registers: `INPUT` (host-provided key state), `RNG` (advanced once), `TICK` (current count).
2. The CPU executes up to a host-chosen **budget** of instructions (or until it stops).
3. The machine writes `STATUS` and increments the tick counter.

Hosts map ticks to their own time base — the emulator runs one tick per rendered frame; the Solana host runs one (or more) per transaction. Device registers keep updating after the CPU stops, so renderers stay live. Determinism rule: given the same seed, per-tick inputs, and budgets, every host produces the identical machine state.

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
  wait for next tick (poll TICK at 0xFF04 until it changes)
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
