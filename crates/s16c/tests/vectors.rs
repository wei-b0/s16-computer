//! The four acceptance vectors from specs/COMPILER.md, verbatim. Behavioral:
//! compile -> assemble with the stock assembler -> run on the real CPU (or
//! machine, for V4) -> assert the required final state.

use s16_core::{Cpu, State, MEMORY_SIZE};
use s16_machine::{mmio, Machine};

fn build(source: &str) -> Box<[u8; MEMORY_SIZE]> {
    let asm = s16c::compile(source).expect("must compile");
    let image = s16_asm::assemble(&asm).expect("emitted assembly must assemble");
    let mut mem = vec![0u8; MEMORY_SIZE];
    mem[..image.len()].copy_from_slice(&image);
    mem.into_boxed_slice().try_into().unwrap()
}

fn run(source: &str) -> (Cpu, Box<[u8; MEMORY_SIZE]>) {
    let mut mem = build(source);
    let mut cpu = Cpu::new();
    cpu.run(&mut mem, 2_000_000);
    (cpu, mem)
}

fn read16(mem: &[u8; MEMORY_SIZE], addr: u16) -> u16 {
    u16::from_le_bytes([mem[addr as usize], mem[addr as usize + 1]])
}

#[test]
fn v1_loops_and_arithmetic() {
    let (cpu, mem) = run(
        r#"
fn sum_to(n: u16) -> u16 {
    let total: u16 = 0;
    let i: u16 = 1;
    while i <= n { total = total + i; i = i + 1; }
    return total;
}
fn main() { poke(0x8000, sum_to(10)); poke(0x8002, (10 * 11) / 2); halt(); }
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(read16(&mem, 0x8000), 55);
    assert_eq!(read16(&mem, 0x8002), 55);
}

#[test]
fn v2_recursion_and_abi() {
    let (cpu, mem) = run(
        r#"
fn fib(n: u16) -> u16 {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}
fn main() { poke(0x8000, fib(10)); halt(); }
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(read16(&mem, 0x8000), 55);
}

#[test]
fn v3_statics_arrays_signedness() {
    let (cpu, mem) = run(
        r#"
static A: [u16; 5] = [10, 20, 30, 40, 50];
static ACC: u16 = 0;

fn main() {
    let i: u16 = 0;
    while i < 5 { ACC = ACC + A[i]; i = i + 1; }
    let d: i16 = -3;
    if d < 0 { ACC = ACC + 1; }
    poke(0x8000, ACC);
    halt();
}
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(read16(&mem, 0x8000), 151);
}

#[test]
fn v4_devices_walk_program() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../programs/walk.s16c"
    ))
    .unwrap();
    let mut mem = build(&source);
    let mut machine = Machine::new(1);

    // A generous per-tick budget: the program draws then polls TICK.
    machine.tick(&mut mem, 0, 50_000);
    let at = |x: u16, y: u16| mmio::cell_addr(y, x) as usize;
    assert_eq!(mem[at(20, 12)], b'@', "drawn at the start position");
    assert_eq!(mem[at(20, 12) + 1], 0x0A, "green attribute");

    // Hold Right for three ticks: the '@' walks right one cell per tick.
    for _ in 0..3 {
        machine.tick(&mut mem, mmio::KEY_RIGHT, 50_000);
    }
    machine.tick(&mut mem, 0, 50_000);
    assert_eq!(mem[at(23, 12)], b'@', "moved right three cells");
    assert_eq!(mem[at(20, 12)], b' ', "old position erased");
    assert_eq!(machine.cpu.state, State::Running, "game loop never halts");
}
