# S16 Emulator Specification

Defines the **contract** of `s16-emulator`: how it loads a program, the two
execution modes it must provide, and what it must show the user in each.
Device behavior itself (framebuffer format, INPUT bits, RNG, TICK) is defined
in [MEMORY_MAP.md](MEMORY_MAP.md) and the tick model in
[MEMORY_MAP.md#machine-tick](MEMORY_MAP.md#machine-tick); this document
covers how the emulator drives that model and presents it as a program.
Implementation: [crates/s16-emulator](../crates/s16-emulator/), built on
[crates/s16-machine](../crates/s16-machine/).

## Image Loading

Both modes load the input file identically: bytes are copied verbatim to
memory starting at `0x0000`, matching reset (see
[ARCHITECTURE.md#reset](ARCHITECTURE.md#reset)); the rest of memory is zero.
An image larger than 64 KB is rejected before execution starts.

## CLI Contract

```
s16-emulator [--tui] [--seed N] [--budget N] <image>
```

| Flag | Default | Meaning |
|------|---------|---------|
| `--tui` | off | Run the interactive machine (below) instead of batch mode |
| `--seed N` | `0xACE1` | RNG seed handed to the machine (decimal or `0x`-hex); must fit 16 bits |
| `--budget N` | `10000` | Instructions executed per tick in `--tui` mode; must be nonzero |

Missing or unreadable image, or a bad flag value, is a usage error: message
on stderr, exit code 1, nothing executed.

## Batch Mode (default)

Runs the CPU alone — no devices are serviced, matching plain `s16-core`
semantics — until it stops or a large step budget (10^8 instructions) is
exhausted, then prints the final state and exits.

The one narrow exception (D025): after the CPU stops, the debug console
buffer (`DEBUG_LEN`/`DEBUG_BUF`, [MEMORY_MAP.md](MEMORY_MAP.md#debug-console-0xf800f901))
is read once, directly from memory — no ticking, no other device serviced —
so a simple non-interactive program (`debug::print_u16(...); halt();`) can
still print a result without `--tui`.

Required output, in order:

1. One status line naming why execution stopped:
   - `halted after N instructions` (`HALT`)
   - `hit BRK after N instructions` (`BRK`)
   - `waiting for input after N instructions (nothing will wake it in batch
     mode)` (`WAIT` — batch mode never writes `INPUT`, so this is terminal)
   - `FAULT <code> (<name>) at PC=0x____ after N instructions` (a fault —
     PC is the faulting instruction, per
     [INSTRUCTION_SET.md#execution-states](INSTRUCTION_SET.md#execution-states))
   - `still running after N instructions (step budget hit)` (likely an
     infinite loop)
2. An `N instructions in T ms (~R IPS)` line, timed around the run itself —
   the first place the emulator reports an actual execution rate rather than
   just an instruction count.
3. If the debug console buffer is non-empty: a `debug output:` line,
   followed by its contents as text.
4. `PC=0x____  SP=0x____  FLAGS=0b____`
5. All 16 registers, `R0`–`R15`, hex, in reading order.

**Exit code:** 0 for `Halted` or `DebugHalt`; 1 for `Faulted`, `Waiting`,
`WaitingTick`, or exhausting the step budget while still `Running`. Scripts
and CI may rely on this to detect a program that didn't finish cleanly.

## Interactive Mode (`--tui`)

Runs the full machine from `s16-machine`: one **tick** per rendered frame, at
a fixed ~30 ticks/second (33 ms/frame). Each tick: collect pending keyboard
events, call `Machine::tick(mem, input_mask, budget)` (which services
INPUT/RNG/TICK, executes up to `budget` instructions, and updates STATUS —
see [MEMORY_MAP.md#machine-tick](MEMORY_MAP.md#machine-tick)), then redraw.
Requires an interactive terminal on stdout; refuses to start otherwise
(exit code 1) rather than producing garbled output.

### Rendering

Every framebuffer cell (`0xF000` base, per
[MEMORY_MAP.md's Framebuffer section](MEMORY_MAP.md)) is drawn each frame:

- **Character byte:** printable ASCII (`0x20`–`0x7E`) renders as itself;
  `0x00` renders as a space; anything else renders as `?` — no raw control
  bytes reach the terminal.
- **Attribute byte:** bits 0–3 select the foreground color, bits 4–6 the
  background, bit 7 enables blink, mapped onto standard ANSI colors/attributes.

A one-line status bar below the grid shows the tick count, that tick's
instructions-executed/budget, a running instructions-per-second figure
(total instructions executed so far divided by wall-clock time since the
session started), and the current CPU state (`running`, `waiting - press a
key to resume`, `waiting for next tick`, `halted`, `BRK - press r to
resume`, or the fault line from batch mode) — or, for any tick where the
program wrote debug-console output (D025), that text in place of everything
else. There is no scrollback: the bar always shows the most recent tick's
debug text, not a running log — a known v1 limitation. Because a `WAIT`ing
or `WAITTICK`ing CPU executes nothing until woken (D026, D028), its
steps-executed figure and its contribution to the running IPS both drop to
`0` for as long as it stays parked — this is what makes IPS a meaningful
number instead of a constant `budget * tick_rate`. In particular, a game
loop that paces itself with `time::wait_next()` (which compiles to
`WAITTICK`) now reports its true per-frame instruction cost instead of
always reading `budget/budget` — see D028.

### Input

Keys map to the INPUT bitmask bits defined in
[MEMORY_MAP.md](MEMORY_MAP.md#system-devices-0xff00-0xffff):

| Bit | Key | Alternate |
|-----|-----|-----------|
| Up | `↑` | `w` |
| Down | `↓` | `s` |
| Left | `←` | `a` |
| Right | `→` | `d` |
| A | `z` | `Space` |
| B | `x` | |
| Start | `Enter` | |
| Select | `Tab` | |

Because most terminals do not reliably report key-release events, a key must
be treated as **held** for a short window (≥ 100 ms) after its most recent
press/repeat, then release automatically — this is an implementation
allowance, not a device-level guarantee; `s16-machine`'s `INPUT` semantics
only require *some* bitmask of currently-considered-held keys each tick.

### Control keys

`Esc` and `Ctrl+C` end the session immediately (not written to any register).
`r` resumes execution when the CPU is in `DebugHalt` (i.e. stopped at `BRK`)
by setting it back to `Running`; it has no effect otherwise. A CPU in
`Waiting` needs no dedicated key: it wakes on the same key presses that feed
the `INPUT` bitmask (D026) — any mapped key (see the table above) wakes it
on the next tick, no separate "resume" gesture required. A CPU in
`WaitingTick` needs no key at all — it wakes automatically on the very next
tick regardless of input (D028).

### Exit

On exit (quit key, or the CPU reaching `Halted`/`Faulted`), the terminal is
restored to its prior state (raw mode off, alternate screen exited, cursor
shown) before anything else is printed. Then, in the same format as batch
mode:

1. A status line reporting why the session ended (`halted` / `stopped at
   BRK` / `waiting for input` / `FAULTED` / `interrupted while running`),
   the tick count, and total instructions executed.
2. The same register dump as batch mode.

**Exit code:** 0 unless the CPU ended `Faulted`, matching batch mode's rule.
