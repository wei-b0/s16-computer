# S16 Instruction Set

The complete v1 ISA reference for S16. Every instruction lists its mnemonic, operands, semantics, effect on FLAGS, and encoding.

See also: [ARCHITECTURE.md](ARCHITECTURE.md), [MEMORY_MAP.md](MEMORY_MAP.md), [ASSEMBLY.md](ASSEMBLY.md).

## Global Rules

- **Word size:** 16 bits. All registers and ALU operations are 16-bit.
- **Memory:** byte-addressable, **little-endian**, 64 KB (addresses `0x0000–0xFFFF`).
- **Alignment:** 16-bit loads/stores (`LDW`/`STW`, `PUSH`/`POP`, `CALL`/`RET`) require 2-byte-aligned addresses. Instructions are 4 bytes and must be 4-byte aligned. A misaligned access faults.
- **Instruction fetch:** each instruction is one 32-bit little-endian word at PC. After a non-branching instruction, PC advances by 4.
- **Immediates:** the 16-bit immediate field holds a full 16-bit value — no split-immediate/LUI mechanism is needed.
- **FLAGS:** only `CMP` and `CMPI` write FLAGS. No other instruction touches them.
- **Control flow is PC-relative:** `JMP`, `CALL`, and all branches take a signed 16-bit offset measured in *instructions*: `target = PC + 4 + offset*4`. Only `JR` is absolute (for computed jumps and jump tables).

## Encoding Formats

Instructions are 32 bits. Fields are defined on the 32-bit instruction word (which is stored little-endian in memory):

```
R-type:  [opcode:8][rd:4][rs1:4][rs2:4][unused:12]
         bits 31-24  23-20  19-16  15-12   11-0

I-type:  [opcode:8][rd:4][rs1:4][imm16:16]
         bits 31-24  23-20  19-16   15-0

J-type:  [opcode:8][unused:8][imm16:16]
         bits 31-24    23-16     15-0
```

- Unused fields must be encoded as zero. Non-zero unused fields are reserved (v1 CPUs ignore them; do not rely on this).
- `imm16` is sign-extended where the instruction says *signed*, otherwise used as a raw 16-bit value.
- Any opcode not listed below is illegal and faults.

## FLAGS Register

| Bit | Flag | Meaning (set by CMP/CMPI computing `a - b`) |
|-----|------|---------------------------------------------|
| 0 | Z | result is zero (`a == b`) |
| 1 | N | bit 15 of result (negative, signed view) |
| 2 | C | borrow occurred (`a < b` unsigned) |
| 3 | V | signed overflow of the subtraction |

Bits 4–15 are reserved and read as zero.

## Instructions

### System

| Opcode | Mnemonic | Format | Semantics |
|--------|----------|--------|-----------|
| `0x00` | `NOP` | R (all fields 0) | Do nothing. |
| `0x01` | `HALT` | R | CPU enters **Halted** state. Program has finished. |
| `0x02` | `BRK` | R | CPU enters **DebugHalt** state. Emulators/debuggers stop here; on-chain, treated like HALT with a debug status. |
| `0x03` | `WAIT` | R | CPU enters **Waiting** state: parked until the host wakes it on a nonzero `INPUT` (D026). Execution for the current tick/step budget stops immediately, without spending the rest of it. |
| `0x04` | `WAITTICK` | R | CPU enters **WaitingTick** state: parked until the next machine tick, unconditionally (D028). Execution for the current tick/step budget stops immediately, without spending the rest of it. |

### Data Movement

| Opcode | Mnemonic | Format | Semantics |
|--------|----------|--------|-----------|
| `0x10` | `MOV rd, rs1` | R | `rd = rs1` |
| `0x11` | `LDI rd, #imm` | I (rs1=0) | `rd = imm16` |
| `0x12` | `LDW rd, [rs1+imm]` | I | `rd = mem16[rs1 + signext(imm)]` |
| `0x13` | `LDB rd, [rs1+imm]` | I | `rd = zeroext(mem8[rs1 + signext(imm)])` |
| `0x14` | `STW [rs1+imm], rd` | I | `mem16[rs1 + signext(imm)] = rd` |
| `0x15` | `STB [rs1+imm], rd` | I | `mem8[rs1 + signext(imm)] = rd & 0xFF` |

Effective addresses are computed mod 0x10000. `LDW`/`STW` fault on odd effective addresses.

### ALU — Register

All R-type: `rd = rs1 op rs2`. Results are truncated to 16 bits. FLAGS are not affected.

| Opcode | Mnemonic | Semantics |
|--------|----------|-----------|
| `0x20` | `ADD rd, rs1, rs2` | `rd = rs1 + rs2` |
| `0x21` | `SUB rd, rs1, rs2` | `rd = rs1 - rs2` |
| `0x22` | `MUL rd, rs1, rs2` | `rd = (rs1 * rs2) & 0xFFFF` (low 16 bits) |
| `0x23` | `DIV rd, rs1, rs2` | `rd = rs1 / rs2` (unsigned). Faults if `rs2 == 0`. |
| `0x24` | `MOD rd, rs1, rs2` | `rd = rs1 % rs2` (unsigned). Faults if `rs2 == 0`. |
| `0x25` | `AND rd, rs1, rs2` | `rd = rs1 & rs2` |
| `0x26` | `OR rd, rs1, rs2` | `rd = rs1 \| rs2` |
| `0x27` | `XOR rd, rs1, rs2` | `rd = rs1 ^ rs2` |
| `0x28` | `NOT rd, rs1` | `rd = ~rs1` (rs2 unused) |
| `0x29` | `SHL rd, rs1, rs2` | `rd = rs1 << rs2` |
| `0x2A` | `SHR rd, rs1, rs2` | `rd = rs1 >> rs2` (logical, zero fill) |
| `0x2B` | `SAR rd, rs1, rs2` | `rd = rs1 >> rs2` (arithmetic, sign fill) |

Shift amounts are taken from the full 16-bit register value. Amounts ≥ 16: `SHL`/`SHR` produce 0; `SAR` produces `0x0000` or `0xFFFF` depending on the sign bit of rs1.

DIV/MOD are unsigned in v1. Signed division, if ever needed, is a library routine.

### ALU — Immediate

All I-type: `rd = rs1 op imm16`. FLAGS are not affected.

| Opcode | Mnemonic | Semantics |
|--------|----------|-----------|
| `0x30` | `ADDI rd, rs1, #imm` | `rd = rs1 + imm16` |
| `0x31` | `SUBI rd, rs1, #imm` | `rd = rs1 - imm16` |
| `0x32` | `ANDI rd, rs1, #imm` | `rd = rs1 & imm16` |
| `0x33` | `ORI rd, rs1, #imm` | `rd = rs1 \| imm16` |
| `0x34` | `XORI rd, rs1, #imm` | `rd = rs1 ^ imm16` |
| `0x35` | `SHLI rd, rs1, #imm` | `rd = rs1 << imm16` |
| `0x36` | `SHRI rd, rs1, #imm` | `rd = rs1 >> imm16` (logical) |
| `0x37` | `SARI rd, rs1, #imm` | `rd = rs1 >> imm16` (arithmetic) |

Shift-amount rules match the register forms.

### Compare

The only instructions that write FLAGS. The subtraction result is discarded.

| Opcode | Mnemonic | Format | Semantics |
|--------|----------|--------|-----------|
| `0x40` | `CMP rs1, rs2` | R (rd=0) | FLAGS ← `rs1 - rs2` |
| `0x41` | `CMPI rs1, #imm` | I (rd=0) | FLAGS ← `rs1 - imm16` |

### Control Flow

J-type instructions use `imm16` as a **signed instruction offset**: `target = PC + 4 + offset*4`. An offset of 0 is the next instruction; -1 is the instruction itself. Assemblers compute offsets from labels.

| Opcode | Mnemonic | Format | Semantics |
|--------|----------|--------|-----------|
| `0x50` | `JMP label` | J | `PC = target` |
| `0x51` | `JR rs1` | R (rd=0, rs2=0) | `PC = rs1` (absolute). Faults if not 4-byte aligned. |
| `0x52` | `BEQ label` | J | branch if `Z == 1` |
| `0x53` | `BNE label` | J | branch if `Z == 0` |
| `0x54` | `BLT label` | J | branch if `N != V` (signed less-than) |
| `0x55` | `BGE label` | J | branch if `N == V` (signed greater-or-equal) |
| `0x56` | `BLTU label` | J | branch if `C == 1` (unsigned less-than) |
| `0x57` | `BGEU label` | J | branch if `C == 0` (unsigned greater-or-equal) |
| `0x58` | `CALL label` | J | `SP -= 2; mem16[SP] = PC + 4; PC = target` |
| `0x59` | `RET` | R (all fields 0) | `PC = mem16[SP]; SP += 2`. Faults if the popped address is not 4-byte aligned. |

A branch whose condition is false falls through to `PC + 4`.

### Stack

The stack grows **downward**. Push pre-decrements; pop post-increments.

| Opcode | Mnemonic | Format | Semantics |
|--------|----------|--------|-----------|
| `0x60` | `PUSH rs1` | R (rd=0, rs2=0) | `SP -= 2; mem16[SP] = rs1` |
| `0x61` | `POP rd` | R (rs1=0, rs2=0) | `rd = mem16[SP]; SP += 2` |
| `0x62` | `RSP rd` | R (rs1=0, rs2=0) | `rd = SP` |
| `0x63` | `WSP rs1` | R (rd=0, rs2=0) | `SP = rs1`. Faults if not 2-byte aligned. |

SP arithmetic wraps mod 0x10000; v1 does not detect stack overflow/underflow.

`RSP`/`WSP` (added in ISA v1.1) exist so software can build stack frames:
without them SP is reachable only implicitly through PUSH/POP/CALL/RET, and a
function could never address its stack locals. The compiler's prologue is
`PUSH R15; RSP R15; RSP r; SUBI r,r,#locals; WSP r` — see
[CALLING_CONVENTION.md](CALLING_CONVENTION.md).

## Faults

Any fault moves the CPU to the **Faulted** state with a fault code; execution stops. There are no interrupts or fault handlers in v1 — determinism and simplicity first.

| Code | Fault | Raised by |
|------|-------|-----------|
| 1 | ILLEGAL_OPCODE | fetching an opcode not defined above |
| 2 | UNALIGNED_ACCESS | odd address in LDW/STW/PUSH/POP/CALL/RET; non-4-byte-aligned PC target in JR/RET |
| 3 | DIV_BY_ZERO | DIV or MOD with a zero divisor |

## Execution States

| State | Entered by | Meaning |
|-------|-----------|---------|
| Running | reset | normal execution |
| Waiting | `WAIT` | parked until the host observes an external event (D026); resumes to Running when woken |
| WaitingTick | `WAITTICK` | parked until the next machine tick (D028); always resumes to Running on the following `tick()` call |
| Halted | `HALT` | program finished cleanly |
| DebugHalt | `BRK` | stopped for a debugger; emulators may resume, on-chain it is terminal |
| Faulted | any fault | stopped with a fault code |

PC semantics on stopping: after `HALT`/`BRK`/`WAIT`/`WAITTICK`, PC points past the stopping instruction (so resuming from DebugHalt, or waking from Waiting/WaitingTick, continues naturally). After a fault, PC still points at the faulting instruction, and none of that instruction's register, memory, or SP updates are visible.

## Opcode Map Summary

```
0x00-0x04  system         NOP HALT BRK WAIT WAITTICK
0x10-0x15  data movement  MOV LDI LDW LDB STW STB
0x20-0x2B  alu register   ADD SUB MUL DIV MOD AND OR XOR NOT SHL SHR SAR
0x30-0x37  alu immediate  ADDI SUBI ANDI ORI XORI SHLI SHRI SARI
0x40-0x41  compare        CMP CMPI
0x50-0x59  control flow   JMP JR BEQ BNE BLT BGE BLTU BGEU CALL RET
0x60-0x63  stack          PUSH POP RSP WSP
```

46 instructions total. All other opcode values are illegal.
