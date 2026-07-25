# S16 Calling Convention (v2 — the ABI)

The binding calling convention for S16, hardened from the v1 sketch now that
a compiler exists ([s16c](LANGUAGE.md)). **Compiled code must conform
exactly.** Hand-written assembly is encouraged to conform, and must at the
boundary whenever it calls, or is called by, compiled code; purely internal
hand-written routines may still relax it with a comment (the v1 allowance).

Requires ISA v1.1 (`RSP`/`WSP`, see
[INSTRUCTION_SET.md](INSTRUCTION_SET.md#stack)).

## Register Roles

| Registers | Role | Saved by |
|-----------|------|----------|
| R0–R3 | Arguments 1–4; R0 also carries the return value | caller |
| R4–R11 | Long-lived values | **callee** |
| R12–R14 | Scratch / temporaries | caller |
| R15 | **Frame pointer (FP)** | callee (by the standard prologue) |

Caller-saved: if the caller needs R0–R3 or R12–R14 preserved across a call,
it saves them itself. Callee-saved: a function that touches R4–R11 or R15
must restore them before `RET`.

## Stack Frame

The stack grows downward from `0xF000`. Immediately after the standard
prologue, a frame looks like:

```
higher addresses
  ...
  [FP+6]  6th argument          (pushed by caller, right to left)
  [FP+4]  5th argument
  [FP+2]  return address        (pushed by CALL)
  [FP+0]  caller's saved FP     (pushed by prologue)     <- FP (R15)
  [FP-2]  first local
  [FP-4]  second local
  ...     callee-saved spills, expression temporaries    <- SP
lower addresses
```

- Arguments beyond the fourth are pushed **right to left** before `CALL`, so
  the 5th argument sits at `[FP+4]`; the **caller** removes them after the
  call returns.
- Locals are addressed at fixed negative offsets from FP; FP does not move
  for the lifetime of the call, so offsets stay valid no matter how SP moves
  (pushes, nested calls).
- Word values only: every argument, local, and return value is one 16-bit
  word, 2-byte aligned. SP must be 2-byte aligned at all times (`WSP` faults
  otherwise).

## Standard Prologue and Epilogue

```asm
func:   PUSH R15            ; save caller's frame pointer
        RSP  R15            ; FP = SP
        SUBI R12, R15, #K   ; K = bytes of locals (omit both lines if K = 0)
        WSP  R12
        ; ... push any of R4-R11 the function will use ...

        ; body: locals at [R15-2], [R15-4], ...; args 5+ at [R15+4], ...

        ; ... pop saved R4-R11 in reverse order ...
        WSP  R15            ; drop locals (and anything pushed above them)
        POP  R15            ; restore caller's FP
        RET
```

Because every activation gets a fresh frame on the stack, **recursion works
by construction**.

**Leaf-function allowance:** a function that makes no calls and needs no
stack locals may skip the frame entirely (no FP touch, everything in
registers) — it must still respect the caller/callee-saved split.

## Worked Example

`max(a, b)` per the ABI — a leaf function, then a framed caller:

```asm
; max(R0=a, R1=b) -> R0        leaf: no frame needed
max:    CMP  R0, R1
        BGEU mdone
        MOV  R0, R1
mdone:  RET

; caller with one local at [FP-2]
demo:   PUSH R15
        RSP  R15
        SUBI R12, R15, #2
        WSP  R12
        LDI  R0, #3
        LDI  R1, #7
        CALL max            ; R0 = 7
        STW  [R15-2], R0    ; local = result
        WSP  R15
        POP  R15
        RET
```

## Notes for s16c

The compiler emits exactly this shape: parameters arrive in R0–R3 and are
spilled to locals when their address stability matters, expression
evaluation uses R12–R14 with stack temporaries below the locals, and
`return e` places `e` in R0 before the epilogue. See
[COMPILER.md](COMPILER.md).
