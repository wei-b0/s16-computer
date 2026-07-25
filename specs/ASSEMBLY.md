# S16 Assembly Language

Syntax specification for S16 assembly, targeted by the future assembler. Instruction semantics and encodings live in [INSTRUCTION_SET.md](INSTRUCTION_SET.md).

## Syntax

- **Mnemonics and registers are case-insensitive**; canonical style is uppercase mnemonics (`ADD`, `LDW`) and uppercase registers (`R0`–`R15`).
- **One instruction or directive per line.**
- **Comments** start with `;` and run to end of line.
- **Labels** are `name:` at the start of a line — an identifier (`[A-Za-z_][A-Za-z0-9_]*`) naming the current address. A label may share a line with an instruction.
- **Immediates** are written with `#`: decimal (`#42`), hex (`#0x2A`), negative (`#-1`), a character (`#'A'`), or a label (`#data`, which resolves to the label's 16-bit address).
- **Memory operands** are `[Rn+off]` / `[Rn-off]`; `[Rn]` means `[Rn+0]`.
- **Branch targets** are written as labels; the assembler computes the signed PC-relative instruction offset. Writing a raw numeric offset (`JMP #-7`) is allowed but discouraged.

## Directives

| Directive | Meaning |
|-----------|---------|
| `.org addr` | Set the current assembly address (default `0x0000`). |
| `.word v, ...` | Emit 16-bit little-endian values (numbers or labels). |
| `.byte v, ...` | Emit bytes. |
| `.ascii "str"` | Emit the string's bytes, no terminator. |
| `.asciiz "str"` | Emit the string's bytes plus a zero terminator. |
| `.space n` | Emit `n` zero bytes. |

## Worked Example (Encoding Test Vector)

Sums five words and halts with the result in R0. The hand-assembled hex below is normative — the assembler and emulator must reproduce it exactly, making this program a shared test vector.

```asm
; sum.s16 — R0 = sum of the 5 words at data
        .org 0x0000

start:  LDI  R1, #data      ; R1 = pointer to list
        LDI  R2, #5         ; R2 = count
        LDI  R0, #0         ; R0 = running sum
loop:   CMPI R2, #0
        BEQ  done
        LDW  R3, [R1]
        ADD  R0, R0, R3
        ADDI R1, R1, #2     ; next word
        SUBI R2, R2, #1
        JMP  loop
done:   HALT

data:   .word 10, 20, 30, 40, 50
```

Hand assembly (instruction words shown as 32-bit values; memory stores them little-endian):

| Addr | Word | Source | Notes |
|--------|--------------|--------|-------|
| `0x0000` | `0x1110002C` | `LDI R1, #data` | `data` = `0x002C` |
| `0x0004` | `0x11200005` | `LDI R2, #5` | |
| `0x0008` | `0x11000000` | `LDI R0, #0` | |
| `0x000C` | `0x41020000` | `CMPI R2, #0` | rd unused = 0, rs1 = 2 |
| `0x0010` | `0x52000005` | `BEQ done` | offset = (0x0028 − 0x0014)/4 = **+5** |
| `0x0014` | `0x12310000` | `LDW R3, [R1]` | |
| `0x0018` | `0x20003000` | `ADD R0, R0, R3` | rd=0, rs1=0, rs2=3 |
| `0x001C` | `0x30110002` | `ADDI R1, R1, #2` | |
| `0x0020` | `0x31220001` | `SUBI R2, R2, #1` | |
| `0x0024` | `0x5000FFF9` | `JMP loop` | offset = (0x000C − 0x0028)/4 = **−7** = `0xFFF9` |
| `0x0028` | `0x01000000` | `HALT` | |
| `0x002C` | — | `.word 10, 20, ...` | bytes: `0A 00 14 00 1E 00 28 00 32 00` |

Expected final state: R0 = 150 (`0x0096`), execution state Halted.

Raw memory image (bytes `0x0000–0x0035`, little-endian):

```
2C 00 10 11  05 00 20 11  00 00 00 11  00 00 02 41
05 00 00 52  00 00 31 12  00 30 00 20  02 00 11 30
01 00 22 31  F9 FF 00 50  00 00 00 01  0A 00 14 00
1E 00 28 00  32 00
```
