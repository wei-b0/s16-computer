# Writing S16 Programs

**This is the assembly s16c compiles to, one level down from
[writing-s16c.md](writing-s16c.md).** Most programs — games included — are
better written in s16c; come here if you want to understand what your
compiled programs turn into, or if you need something the standard library
doesn't expose. Full syntax lives in
[specs/ASSEMBLY.md](../specs/ASSEMBLY.md) and instruction semantics in
[specs/INSTRUCTION_SET.md](../specs/INSTRUCTION_SET.md); this page builds up a
program the way you actually write one. Assemble and run each step with:

```sh
cargo run -p s16-asm -- yourfile.s16 && cargo run -p s16-emulator -- yourfile.bin
```

## The smallest program

```asm
        LDI  R0, #42
        HALT
```

Execution starts at address `0x0000` — the first instruction in the file. Every
program needs a `HALT`; without one the CPU runs into whatever bytes come next
(zeroes decode as `NOP`, then it faults or loops forever). Run it: the
emulator prints `R0 =0x002A`.

Things to notice: immediates always start with `#`, and results end up in
registers — the register dump is your program's "output" for now.

## A loop: sum 1..10

There is no loop instruction; you build loops from `CMP` + a conditional
branch:

```asm
        LDI  R1, #10        ; counter
        LDI  R0, #0         ; accumulator
loop:   CMPI R1, #0
        BEQ  done           ; counter == 0 -> exit loop
        ADD  R0, R0, R1
        SUBI R1, R1, #1
        JMP  loop
done:   HALT
```

Result: `R0 =0x0037` (55). The pattern to internalize: **only `CMP`/`CMPI` set
the flags** that branches test. `SUBI R1, R1, #1` does *not* set flags, so you
cannot branch on it directly — compare first, then branch. One `CMP` can feed
several branches in a row.

Signed vs unsigned matters when comparing: `BLT`/`BGE` treat values as signed
(-32768..32767), `BLTU`/`BGEU` as unsigned (0..65535). For addresses and
counts, use the unsigned forms.

## Data: labels are just addresses

```asm
        LDI  R1, #data      ; R1 = address of the list
        LDW  R2, [R1]       ; first word
        LDW  R3, [R1+2]     ; second word (words are 2 bytes)
        HALT

data:   .word 17, 4, 99
```

`#data` is the label's address as an immediate; `[R1+2]` is base-plus-offset
addressing. Put data **after** the code: execution starts at 0 and data before
code would be executed as instructions. Keep word data at even addresses —
`LDW`/`STW` fault on odd ones (`.word` after 4-byte instructions is always
fine).

For byte data (`.byte`, `.ascii`), use `LDB`/`STB`; `LDB` zero-extends the
byte into the register.

## Subroutines: a complete program

`CALL` pushes the return address and jumps; `RET` pops it. Here's max-of-list
as a reusable routine, following the
[calling convention](../specs/CALLING_CONVENTION.md): arguments in R1/R2,
result in R0:

```asm
        LDI  R1, #data
        LDI  R2, #6          ; count
        CALL max
        HALT                 ; R0 = 99

; max(R1 = ptr, R2 = count) -> R0. Clobbers R1-R3.
max:    LDI  R0, #0
mloop:  CMPI R2, #0
        BEQ  mdone
        LDW  R3, [R1]
        CMP  R3, R0
        BLTU mskip           ; R3 < current max -> skip
        MOV  R0, R3
mskip:  ADDI R1, R1, #2
        SUBI R2, R2, #1
        JMP  mloop
mdone:  RET

data:   .word 17, 4, 99, 23, 0, 60
```

Result: `R0 =0x0063` (99). If a routine needs a register beyond scratch space,
preserve it: `PUSH R4` on entry, `POP R4` before `RET` — and document what you
clobber in a comment, like above. The stack lives just below `0xF000` and
grows down; you get it for free, no setup needed.

## Talking to the hardware

Devices are just memory addresses ([specs/MEMORY_MAP.md](../specs/MEMORY_MAP.md)).
Writing a character to the top-left corner of the 40×25 display:

```asm
        LDI  R1, #0xF000     ; framebuffer cell (0,0)
        LDI  R2, #0x0223     ; attribute 0x02, character '#' (0x23)
        STW  [R1], R2
        HALT
```

Run it with `--tui` and the `#` actually appears on screen:

```sh
cargo run -p s16-emulator -- --tui yourfile.bin     # esc quits
```

In `--tui` mode the emulator runs the full machine: the framebuffer renders
each tick, the keyboard feeds `INPUT` (arrows/WASD = d-pad, Z = A, X = B,
Enter = Start), and `RNG`/`TICK` update every tick. A program that never
`HALT`s — reading input, redrawing, pacing itself on `TICK` — is a *machine
loop*, and it's how games work here: see
[programs/asm/spin.s16](../programs/asm/spin.s16) for the minimal example. In batch
mode (no `--tui`) devices stay silent and only the CPU runs.

## Habits that pay off

- End every code path in `HALT` (or a loop you mean to be infinite).
- Data after code; word data on even addresses.
- Comment each routine's contract: inputs, outputs, what it clobbers.
- Stuck? Drop a `BRK` where you want to look around —
  [debugging.md](debugging.md).

Everything here — loops, subroutines, MMIO — is what
[writing-s16c.md](writing-s16c.md) automates: `while`, function calls, and
`put_cell`/`read_input` compile straight down to the patterns above.
