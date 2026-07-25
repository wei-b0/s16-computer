//! Machine-layer tests: device registers behave as specs/MEMORY_MAP.md says,
//! and the whole loop is deterministic. Test programs are assembled from
//! source with s16-asm, so these are toolchain integration tests too.

use s16_core::{State, MEMORY_SIZE};
use s16_machine::{mmio, status_word, Machine, Xorshift16};

fn load(source: &str) -> Box<[u8; MEMORY_SIZE]> {
    let image = s16_asm::assemble(source).expect("test program must assemble");
    let mut mem = vec![0u8; MEMORY_SIZE];
    mem[..image.len()].copy_from_slice(&image);
    mem.into_boxed_slice().try_into().unwrap()
}

fn read16(mem: &[u8; MEMORY_SIZE], addr: u16) -> u16 {
    u16::from_le_bytes([mem[addr as usize], mem[addr as usize + 1]])
}

#[test]
fn program_reads_input_rng_and_tick() {
    // Copy the three device registers into R1-R3, then halt.
    let mut mem = load(
        r#"
        LDI  R4, #0xFF00
        LDW  R1, [R4]       ; INPUT
        LDW  R2, [R4+2]     ; RNG
        LDW  R3, [R4+4]     ; TICK
        HALT
"#,
    );
    let mut machine = Machine::new(42);
    machine.tick(&mut mem, mmio::KEY_LEFT | mmio::KEY_A, 1_000);

    assert_eq!(machine.cpu.state, State::Halted);
    assert_eq!(machine.cpu.regs[1], mmio::KEY_LEFT | mmio::KEY_A);
    assert_eq!(machine.cpu.regs[2], Xorshift16::new(42).advance());
    assert_eq!(machine.cpu.regs[3], 0, "first tick is 0");
}

#[test]
fn devices_update_each_tick() {
    let mut mem = load("loop: JMP loop"); // never halts, never reads devices
    let mut machine = Machine::new(1);

    let mut rng_values = Vec::new();
    for n in 0..5u16 {
        machine.tick(&mut mem, n, 10);
        assert_eq!(read16(&mem, mmio::INPUT), n, "INPUT follows the host");
        assert_eq!(read16(&mem, mmio::TICK), n, "TICK counts ticks");
        rng_values.push(read16(&mem, mmio::RNG));
    }
    rng_values.dedup();
    assert_eq!(rng_values.len(), 5, "RNG advances every tick");
    assert_eq!(machine.ticks, 5);
}

#[test]
fn budget_bounds_each_tick() {
    let mut mem = load("loop: JMP loop");
    let mut machine = Machine::new(1);
    assert_eq!(machine.tick(&mut mem, 0, 7), 7, "budget respected");
    assert_eq!(machine.cpu.state, State::Running, "slice ended, not stopped");
}

#[test]
fn status_reflects_state_and_fault() {
    let mut mem = load("LDI R1, #3\nLDI R2, #0\nDIV R3, R1, R2");
    let mut machine = Machine::new(1);

    let steps = machine.tick(&mut mem, 0, 1_000);
    assert_eq!(steps, 3);
    assert_eq!(machine.cpu.state, State::Faulted(s16_core::Fault::DivByZero));
    // low byte 3 = Faulted, high byte 3 = DIV_BY_ZERO
    assert_eq!(read16(&mem, mmio::STATUS), 0x0303);
    assert_eq!(read16(&mem, mmio::STATUS), status_word(machine.cpu.state));

    // The machine stays serviceable after a stop: devices tick, CPU doesn't.
    assert_eq!(machine.tick(&mut mem, 0, 1_000), 0);
    assert_eq!(read16(&mem, mmio::TICK), 1);
}

#[test]
fn deterministic_replay() {
    let src = r#"
        LDI  R5, #0xFF00
        LDI  R1, #0
loop:   LDW  R2, [R5+2]     ; RNG
        ADD  R1, R1, R2     ; mix into R1
        LDW  R3, [R5+4]     ; TICK
        CMPI R3, #20
        BLTU loop           ; spin until tick 20
        HALT
"#;
    let run = || {
        let mut mem = load(src);
        let mut machine = Machine::new(0xACE1);
        for t in 0..40u16 {
            machine.tick(&mut mem, t / 3, 25); // varying input, small budget
        }
        (machine.cpu.regs, machine.cpu.state, machine.ticks)
    };
    assert_eq!(run(), run(), "same seed + inputs + budgets => same machine");
}

#[test]
fn hello_program_fills_framebuffer() {
    let image = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../programs/asm/hello.s16"
    ))
    .unwrap();
    let mut mem = load(&image);
    let mut machine = Machine::new(1);
    machine.tick(&mut mem, 0, 10_000);

    assert_eq!(machine.cpu.state, State::Halted);
    for (i, ch) in b"HELLO, S16!".iter().enumerate() {
        let addr = mmio::cell_addr(0, i as u16);
        assert_eq!(mem[addr as usize], *ch, "character byte of cell {i}");
        assert_eq!(mem[addr as usize + 1], 0x0F, "attribute byte of cell {i}");
    }
}

#[test]
fn spin_program_animates_from_tick() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../programs/asm/spin.s16"
    ))
    .unwrap();
    let mut mem = load(&source);
    let mut machine = Machine::new(1);
    let cell = mmio::cell_addr(12, 20) as usize;

    machine.tick(&mut mem, 0, 100); // tick 0 -> frame 0
    assert_eq!(mem[cell], b'|');
    assert_eq!(mem[cell + 1], 0x0A, "green attribute");

    for _ in 0..8 {
        machine.tick(&mut mem, 0, 100); // through tick 8 -> frame 1
    }
    assert_eq!(mem[cell], b'/', "frame advances every 8 ticks");
    assert_eq!(machine.cpu.state, State::Running, "a machine loop never halts");
}

#[test]
fn debug_console_drains_and_resets_each_tick() {
    // Poke "Hi" into DEBUG_BUF and set DEBUG_LEN=2, then halt.
    let mut mem = load(&format!(
        r#"
        LDI  R1, #0x{buf:04X}
        LDI  R2, #0x48       ; 'H'
        STB  [R1], R2
        LDI  R2, #0x69       ; 'i'
        STB  [R1+1], R2
        LDI  R1, #0x{len:04X}
        LDI  R2, #2
        STW  [R1], R2
        HALT
"#,
        buf = mmio::DEBUG_BUF,
        len = mmio::DEBUG_LEN,
    ));
    let mut machine = Machine::new(1);

    machine.tick(&mut mem, 0, 100);
    assert_eq!(machine.debug_output(), b"Hi");
    assert_eq!(read16(&mem, mmio::DEBUG_LEN), 0, "DEBUG_LEN resets after drain");

    // No new writes: next tick's debug_output is empty (replace, not accumulate).
    machine.tick(&mut mem, 0, 100);
    assert!(machine.debug_output().is_empty());
}

#[test]
fn debug_console_output_is_capped_at_capacity() {
    let mut mem = load(&format!(
        "LDI R1, #0x{len:04X}\nLDI R2, #9000\nSTW [R1], R2\nHALT",
        len = mmio::DEBUG_LEN,
    ));
    let mut machine = Machine::new(1);
    machine.tick(&mut mem, 0, 100);
    assert_eq!(machine.debug_output().len(), mmio::DEBUG_BUF_CAP);
}

#[test]
fn zero_seed_is_valid() {
    let mut a = Xorshift16::new(0);
    let mut b = Xorshift16::new(1);
    assert_eq!(a.advance(), b.advance());
    assert_ne!(Xorshift16::new(5).advance(), 0, "state never becomes zero");
}
