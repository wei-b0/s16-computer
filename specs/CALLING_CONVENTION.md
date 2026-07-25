# S16 Calling Convention (v1)

A lightweight **software convention** for hand-written assembly. Nothing here is enforced by hardware, and this is deliberately *not* a strict ABI — rigid caller-saved/callee-saved rules can wait until a compiler exists and needs them.

## Register Roles

| Registers | Role |
|-----------|------|
| R0–R3 | Arguments and return value. R0 carries the return value. |
| R4–R11 | General purpose. |
| R12–R15 | Temporaries / scratch. |

Guidance (not law): treat R12–R15 as freely clobbered by any call; if a routine uses R4–R11 for long-lived state, say so in a comment at its definition. With hand-written assembly, a comment is the ABI.

More than 4 arguments: push the extras on the stack before `CALL` (caller cleans up after return). Rare in practice at this scale.

## Stack and Calls

The stack grows downward from `0xF000` (SP reset value). `CALL` pushes the return address; `RET` pops it into PC:

```
CALL f:   SP -= 2; mem16[SP] = PC + 4; PC = f
RET:      PC = mem16[SP]; SP += 2
```

A routine that needs stack space simply moves SP and restores it before `RET`:

```asm
; strlen-style routine: R0 = length of zero-terminated bytes at R0
count:  PUSH R4            ; preserving a long-lived register we borrow
        MOV  R4, R0
        LDI  R0, #0
next:   LDB  R12, [R4+0]   ; R12: scratch, no need to save
        CMPI R12, #0
        BEQ  done
        ADDI R0, R0, #1
        ADDI R4, R4, #1
        JMP  next
done:   POP  R4
        RET
```

Rules that keep this sane:

1. SP must be restored to its `CALL`-time value before `RET` (the return address must be on top of the stack).
2. Anything pushed inside a routine must be popped by that routine.
3. There is no frame pointer register in v1; routines with complex frames may dedicate one of R4–R11 by local convention.

## Future

When a compiler targets S16, this document gets superseded by a real ABI (fixed caller/callee-saved split, frame layout, varargs). Until then, simplicity wins.
