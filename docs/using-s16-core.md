# Using s16-core From Rust

How to embed the S16 CPU in your own Rust program — a custom test harness, a
debugger, a fuzzer, or a new host. This is exactly how the emulator and the
future Solana program use it.

## Run a program to completion

```rust
use s16_core::{Cpu, State, MEMORY_SIZE};

let program = std::fs::read("programs/asm/sum.bin")?;

let mut mem = vec![0u8; MEMORY_SIZE];
mem[..program.len()].copy_from_slice(&program);
let mem: &mut [u8; MEMORY_SIZE] = (&mut mem[..]).try_into().unwrap();

let mut cpu = Cpu::new();                 // spec reset state: PC=0, SP=0xF000
cpu.run(mem, 1_000_000);                  // step budget guards against loops

assert_eq!(cpu.state, State::Halted);
println!("result: {}", cpu.regs[0]);
```

The CPU never owns memory — you hand it a `&mut [u8; 65536]` each call. That's
deliberate: hosts decide where memory lives (a `Vec` here, account data
on-chain). Everything on `Cpu` is a public field; read or overwrite `regs`,
`pc`, `sp`, `flags`, `state` freely between steps.

You can assemble in-process instead of shelling out:

```rust
let image = s16_asm::assemble(std::fs::read_to_string("prog.s16")?.as_str())?;
```

## Step-by-step control

`run` is just a loop over `step`. Stepping yourself is how you build
debuggers, tracers, or device emulation:

```rust
use s16_core::State;

while cpu.state == State::Running {
    cpu.step(mem);

    // Between steps you own the machine: trace it...
    println!("PC={:#06X} R0={:#06X}", cpu.pc, cpu.regs[0]);

    // ...or implement MMIO devices by editing memory (specs/MEMORY_MAP.md):
    // mem[0xFF00] = key_bitmask;                       // INPUT
    // mem[0xFF02..0xFF04].copy_from_slice(&rng_next.to_le_bytes());  // RNG
}
```

This between-steps hook is the entire device model: the core treats all 64 KB
as plain memory, and hosts breathe life into the MMIO addresses. You usually
don't do this by hand — the `s16-machine` crate packages the standard tick
loop (INPUT/RNG/TICK each tick, budgeted CPU slice, STATUS update) with the
device semantics pinned in specs/MEMORY_MAP.md:

```rust
let mut machine = s16_machine::Machine::new(seed);
machine.tick(mem, key_bitmask, 10_000);   // one tick: devices + CPU slice
```

Drive `step` directly only when you need something nonstandard (tracers,
custom devices). Either way, keep what you feed the registers deterministic if
you care about matching on-chain execution.

## Handle every stop reason

```rust
use s16_core::{Fault, State};

match cpu.state {
    State::Running => { /* step budget ran out */ }
    State::Halted => { /* clean HALT */ }
    State::DebugHalt => {
        // BRK: PC is already past it, so a debugger can inspect state
        // and simply resume:
        cpu.state = State::Running;
    }
    State::Faulted(f) => {
        // PC still points AT the faulting instruction; none of its
        // effects were applied. Not resumable.
        eprintln!("fault {} ({f:?}) at {:#06X}", f.code(), cpu.pc);
    }
}
```

## Writing tests against the CPU

The trick used throughout this repo: assemble a snippet, run it, assert on
final state. Great for testing S16 routines you plan to use in a bigger
program:

```rust
#[test]
fn max_routine_works() {
    let image = s16_asm::assemble(include_str!("max.s16")).unwrap();
    let mut mem = vec![0u8; s16_core::MEMORY_SIZE];
    mem[..image.len()].copy_from_slice(&image);
    let mem: &mut [u8; s16_core::MEMORY_SIZE] = (&mut mem[..]).try_into().unwrap();

    let mut cpu = s16_core::Cpu::new();
    cpu.run(mem, 10_000);

    assert_eq!(cpu.state, s16_core::State::Halted);
    assert_eq!(cpu.regs[0], 99);
}
```

Assert on memory too — `mem` after the run is the machine's memory, so a
routine that builds a string or fills a framebuffer region can be checked
byte-for-byte.

## Ground rules for new hosts

- Never add host behavior *inside* the core; drive it from outside via memory.
- Keep anything you feed the MMIO registers deterministic (seeded PRNG, not
  `rand::random`) if the run must be reproducible or match the chain.
- Budget your `run` calls; untrusted images can always loop forever.
