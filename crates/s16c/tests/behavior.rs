//! Targeted semantics tests beyond the spec vectors: each compiles a small
//! program, runs it on the real CPU, and asserts memory poked at 0x8000+.

use s16_core::{Cpu, State, MEMORY_SIZE};

fn run(source: &str) -> (Cpu, Box<[u8; MEMORY_SIZE]>) {
    let asm = s16c::compile(source).expect("must compile");
    let image = s16_asm::assemble(&asm).expect("emitted assembly must assemble");
    let mut mem = vec![0u8; MEMORY_SIZE];
    mem[..image.len()].copy_from_slice(&image);
    let mut mem: Box<[u8; MEMORY_SIZE]> = mem.into_boxed_slice().try_into().unwrap();
    let mut cpu = Cpu::new();
    cpu.run(&mut mem, 2_000_000);
    assert_eq!(cpu.state, State::Halted, "program should halt cleanly");
    (cpu, mem)
}

fn r16(mem: &[u8; MEMORY_SIZE], addr: u16) -> u16 {
    u16::from_le_bytes([mem[addr as usize], mem[addr as usize + 1]])
}

#[test]
fn wrapping_arithmetic() {
    let (_, mem) = run(
        "fn main() { var x: u16 = 0xFFFF; poke(0x8000, x + 3); poke(0x8002, 0 - 1 + 0); halt(); }",
    );
    assert_eq!(r16(&mem, 0x8000), 2);
    assert_eq!(r16(&mem, 0x8002), 0xFFFF);
}

#[test]
fn short_circuit_skips_side_effects() {
    let (_, mem) = run(
        r#"
static HITS: u16 = 0;
fn touch() -> bool { HITS = HITS + 1; return true; }
fn main() {
    var a: bool = false && touch();   // rhs must not run
    var b: bool = true || touch();    // rhs must not run
    var c: bool = true && touch();    // rhs runs once
    if a == b { }
    if c { }
    poke(0x8000, HITS);
    halt();
}
"#,
    );
    assert_eq!(r16(&mem, 0x8000), 1);
}

#[test]
fn shadowing_and_scopes() {
    let (_, mem) = run(
        r#"
fn main() {
    var x: u16 = 1;
    if true {
        var x: u16 = 2;
        poke(0x8000, x);
    }
    poke(0x8002, x);
    halt();
}
"#,
    );
    assert_eq!(r16(&mem, 0x8000), 2);
    assert_eq!(r16(&mem, 0x8002), 1);
}

#[test]
fn casts_and_signedness() {
    let (_, mem) = run(
        r#"
fn main() {
    var a: u16 = 0xFFFF;
    var b: i16 = a as i16;            // reinterpret: -1
    if b < 0 { poke(0x8000, 1); }     // signed compare: taken
    if a > 0 { poke(0x8002, 1); }     // unsigned compare: taken
    var c: u16 = 0x8000;
    poke(0x8004, c >> 1);             // logical: 0x4000
    var d: i16 = c as i16;
    poke(0x8006, (d >> 1) as u16);    // arithmetic: 0xC000
    halt();
}
"#,
    );
    assert_eq!(r16(&mem, 0x8000), 1);
    assert_eq!(r16(&mem, 0x8002), 1);
    assert_eq!(r16(&mem, 0x8004), 0x4000);
    assert_eq!(r16(&mem, 0x8006), 0xC000);
}

#[test]
fn unary_ops() {
    let (_, mem) = run(
        r#"
fn main() {
    var x: i16 = 5;
    poke(0x8000, (-x) as u16);        // 0xFFFB
    var m: u16 = 0x00F0;
    poke(0x8002, !m);                 // bitwise: 0xFF0F
    var t: bool = true;
    if !t { } else { poke(0x8004, 1); }
    halt();
}
"#,
    );
    assert_eq!(r16(&mem, 0x8000), 0xFFFB);
    assert_eq!(r16(&mem, 0x8002), 0xFF0F);
    assert_eq!(r16(&mem, 0x8004), 1);
}

#[test]
fn six_parameter_function_uses_stack_args() {
    let (_, mem) = run(
        r#"
fn weigh(a: u16, b: u16, c: u16, d: u16, e: u16, f: u16) -> u16 {
    return a + b * 2 + c * 3 + d * 4 + e * 5 + f * 6;
}
fn main() { poke(0x8000, weigh(1, 2, 3, 4, 5, 6)); halt(); }
"#,
    );
    // 1 + 4 + 9 + 16 + 25 + 36 = 91
    assert_eq!(r16(&mem, 0x8000), 91);
}

#[test]
fn nested_calls_do_not_clobber_arguments() {
    let (_, mem) = run(
        r#"
fn add(a: u16, b: u16) -> u16 { return a + b; }
fn main() {
    // arg staging must survive the nested calls
    poke(0x8000, add(add(1, 2), add(3, 4)));
    halt();
}
"#,
    );
    assert_eq!(r16(&mem, 0x8000), 10);
}

#[test]
fn loop_break_continue() {
    let (_, mem) = run(
        r#"
fn main() {
    var sum: u16 = 0;
    var i: u16 = 0;
    loop {
        i = i + 1;
        if i > 10 { break; }
        if i % 2 == 0 { continue; }
        sum = sum + i;                // odd numbers 1..9
    }
    poke(0x8000, sum);
    halt();
}
"#,
    );
    assert_eq!(r16(&mem, 0x8000), 25);
}

#[test]
fn div_mod_and_consts() {
    let (_, mem) = run(
        r#"
const BASE: u16 = 0x8000;
const SEVEN: u16 = 3 + 4;
fn main() {
    var n: u16 = 100;
    poke(BASE, n / SEVEN);        // 14
    poke(BASE + 2, n % SEVEN);    // 2
    halt();
}
"#,
    );
    assert_eq!(r16(&mem, 0x8000), 14);
    assert_eq!(r16(&mem, 0x8002), 2);
}

#[test]
fn i16_array_and_negative_values() {
    let (_, mem) = run(
        r#"
static D: [i16; 3] = [-1, 0, 1];
fn main() {
    var count: u16 = 0;
    var i: u16 = 0;
    while i < 3 {
        if D[i] < 0 { count = count + 1; }
        i = i + 1;
    }
    poke(0x8000, count);
    halt();
}
"#,
    );
    assert_eq!(r16(&mem, 0x8000), 1);
}
