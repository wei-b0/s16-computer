# S16 Assembler Specification

Defines the **contract** of `s16-asm`: what any conforming implementation
must do when it turns [ASSEMBLY.md](ASSEMBLY.md) source into a memory image.
The language itself — syntax, directives, literals — is specified in
[ASSEMBLY.md](ASSEMBLY.md); this document covers assembly-time behavior:
invocation, output format, error reporting, and the encoding contract shared
with [INSTRUCTION_SET.md](INSTRUCTION_SET.md). Implementation:
[crates/s16-asm](../crates/s16-asm/).

## Model

The assembler is a pure function from source text to bytes:

```
assemble(source: &str) -> Result<Vec<u8>, AsmError>
```

Two passes, deterministically:

1. **Pass 1** walks the source once, sizing every statement (instructions are
   always 4 bytes; directive sizes come from their arguments) and recording
   each label's resolved address. `.org` resets the address cursor;
   everything else advances it.
2. **Pass 2** re-walks the source with all labels known and encodes each
   statement. Branch/`CALL` labels resolve to signed PC-relative instruction
   offsets (`(target - (addr + 4)) / 4`, per
   [INSTRUCTION_SET.md](INSTRUCTION_SET.md#control-flow)); `#label`
   immediates resolve to the label's absolute address.

There is no linker, no macro system, and no relocation: one source file
produces one flat image. `.org` is the only placement tool.

**Determinism requirement:** the same source text must always assemble to
the same bytes, on every implementation. This matters beyond convenience —
`ASSEMBLY.md`'s worked example is a normative test vector precisely because
independent assemblers must agree with it byte for byte.

## Encoding Contract

Every instruction encodes exactly as [INSTRUCTION_SET.md](INSTRUCTION_SET.md)
defines: same opcode values, same R/I/J field layout, same
sign/zero-extension rules. An assembler and a CPU implementation must not be
able to disagree about what a mnemonic encodes to — in `s16-asm`, both
depend on the single opcode table in `s16-core`, and the compliance test is
literally: assemble `ASSEMBLY.md`'s example, compare to its hand-assembled
hex, byte for byte; then run the result on the CPU and compare final
register state.

## Output

Success writes a flat binary image: the file's bytes belong at address
`0x0000` (see [MEMORY_MAP.md](MEMORY_MAP.md)). The image ends at the highest
address written by any statement; addresses skipped by `.org` are zero-filled
up to that point. The image must fit in 64 KB — assembling past `0xFFFF` is
an error, not silent truncation or wraparound.

## CLI Contract

```
s16-asm <input> [-o <output>]
```

| Behavior | Contract |
|----------|----------|
| Default output path | input path with its extension replaced by `.bin` |
| On success | image written to the output path; stdout prints `{input} -> {output} ({N} bytes)`; exit code 0 |
| On failure | **no output file is written** (or left from a previous run — never a partial/truncated one); message on stderr; exit code 1 |
| Unreadable input, unwritable output | reported as `s16-asm: cannot read/write {path}: {error}` |

## Error Reporting

Every assembly error carries the **1-indexed source line** that caused it and
is reported as:

```
{input}:{line}: {message}
```

An implementation must reject, with a line number, each of:

- unknown mnemonic or directive
- wrong operand count for a mnemonic
- an immediate missing its `#` prefix, or a raw number where a register is
  expected (and vice versa)
- a value that doesn't fit the target width (16 bits for immediates/words, 8
  bits signed-or-unsigned for `.byte`, 16 bits signed for branch offsets)
- an undefined label, or a label defined twice
- a malformed memory operand (not `[Rn]` / `[Rn+off]` / `[Rn-off]`)
- an instruction whose address is not 4-byte aligned (data placed before
  code without realigning via `.org`/`.space`)
- output that would exceed the 64 KB address space

The exact wording of a message is not part of the contract; that a line
number is attached and that the condition is rejected (not silently
accepted) is.
