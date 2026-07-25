# Debugging S16 Programs

The emulator prints the full machine state whenever the CPU stops. Debugging
on S16 is about making it stop where you want, and reading what it tells you.

## BRK: a breakpoint in one instruction

Insert `BRK` where you want to inspect state:

```asm
        LDI  R0, #42
        BRK                 ; look at the machine here
        LDI  R0, #7
        HALT
```

```
hit BRK after 2 instructions
PC=0x0008  SP=0xF000  FLAGS=0b0000
R0 =0x002A  ...
```

Everything up to the `BRK` has executed; `PC=0x0008` points at the *next*
instruction. Move the `BRK` around and rerun to bisect a bug — assembling and
running is fast enough that this beats staring at code. (Exit code is 0 for
`BRK`, same as `HALT`, so scripts won't treat it as a failure.)

## Reading a fault

```
FAULT 2 (UnalignedAccess) at PC=0x0004 after 2 instructions
PC=0x0004  SP=0xF000  FLAGS=0b0000
R0 =0x0000  R1 =0x1001 ...
```

The PC is the address of the **faulting instruction itself** — nothing from it
was applied. Since instructions are 4 bytes and code normally starts at 0,
`PC ÷ 4` is the instruction's index in your listing: `0x0004 / 4` = the second
instruction. The register dump shows the machine at the moment of the fault —
here `R1 =0x1001`, an odd address, is the culprit being dereferenced.

The three faults and their usual causes:

| Fault | Usually means |
|-------|---------------|
| `1 (IllegalOpcode)` | Execution wandered into data or zero-filled memory — a missing `HALT`, data placed before code, or a wild `JR`. |
| `2 (UnalignedAccess)` | `LDW`/`STW` on an odd address (byte-indexing word data — remember cells and words are 2 bytes), or `JR`/`RET` to a non-4-byte-aligned target — often a smashed stack. |
| `3 (DivByZero)` | `DIV`/`MOD` with a zero divisor — guard with `CMPI divisor, #0` / `BEQ`. |

## "still running"

`still running after 100000000 instructions` means the step budget ran out:
an unintended infinite loop. The printed PC tells you *which* loop the CPU is
stuck in. The classic cause on S16: branching on flags you never set —
arithmetic doesn't touch FLAGS, only `CMP`/`CMPI` do, so a loop whose exit
condition relies on `SUBI` "setting zero" never exits. Compare, then branch.

## Assembler errors

The assembler reports `file:line: message` and writes no output. Frequent ones:

- `immediates start with '#'` — you wrote `LDI R1, 5` instead of `#5`.
- `undefined label` — typo, or you used `#label` syntax where a bare label
  belongs (branches take `BEQ done`, not `BEQ #done`).
- `instruction at 0x... is not 4-byte aligned` — you put odd-sized data
  (`.byte`/`.ascii`) *before* code; move it after, or pad with `.space`.
- `duplicate label` — two definitions; labels are file-global.

## When the registers aren't enough

Have the program write intermediate values into spare registers (R12–R15 are
scratch by convention) right before a `BRK` — a poor man's `printf`. For
anything deeper, drive the CPU from Rust and inspect memory directly:
[using-s16-core.md](using-s16-core.md).
