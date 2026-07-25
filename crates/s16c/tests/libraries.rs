//! The standard libraries (crates/s16c/src/libs/*.s16c), exercised end to
//! end: compile -> assemble -> run/tick on the real CPU/machine. Covers
//! behavior, shadowing, dead-code exclusion, and the `use` mechanism itself
//! (specs/COMPILER.md "Standard Library Merge").

use s16_core::{Cpu, State, MEMORY_SIZE};
use s16_machine::{mmio, Machine};

fn build(source: &str) -> Box<[u8; MEMORY_SIZE]> {
    let asm = s16c::compile(source).unwrap_or_else(|e| panic!("compile: {e}"));
    let image = s16_asm::assemble(&asm).unwrap_or_else(|e| panic!("assemble: {e}"));
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

fn cell(mem: &[u8; MEMORY_SIZE], x: u16, y: u16) -> (u8, u8) {
    let addr = mmio::cell_addr(y, x) as usize;
    (mem[addr], mem[addr + 1])
}

#[test]
fn put_cell_and_get_cell_round_trip() {
    let (cpu, mem) = run(
        r#"
use screen;
fn main() {
    put_cell(5, 3, 'X', GREEN);
    var back: u16 = get_cell(5, 3);
    poke(0x8000, back);
    halt();
}
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cell(&mem, 5, 3), (b'X', 10)); // GREEN = 10
    assert_eq!(u16::from_le_bytes([mem[0x8000], mem[0x8001]]), b'X' as u16);
}

#[test]
fn clear_screen_blanks_every_cell() {
    let (cpu, mem) = run(
        r#"
use screen;
fn main() {
    put_cell(0, 0, '#', WHITE);
    put_cell(39, 24, '#', WHITE);
    clear_screen();
    halt();
}
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cell(&mem, 0, 0), (b' ', 0));
    assert_eq!(cell(&mem, 39, 24), (b' ', 0));
    assert_eq!(cell(&mem, 20, 12), (b' ', 0));
}

#[test]
fn key_helpers_decode_the_input_bitmask() {
    let (cpu, mem) = run(
        r#"
use input;
fn main() {
    var keys: u16 = 21;   // UP | LEFT | A  (bits 0, 2, 4 -> 1+4+16)
    if key_up(keys)    { poke(0x8000, 1); }
    if key_down(keys)  { poke(0x8002, 1); }
    if key_left(keys)  { poke(0x8004, 1); }
    if key_right(keys) { poke(0x8006, 1); }
    if key_a(keys)     { poke(0x8008, 1); }
    halt();
}
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    let r = |a: u16| u16::from_le_bytes([mem[a as usize], mem[a as usize + 1]]);
    assert_eq!(r(0x8000), 1, "up");
    assert_eq!(r(0x8002), 0, "down");
    assert_eq!(r(0x8004), 1, "left");
    assert_eq!(r(0x8006), 0, "right");
    assert_eq!(r(0x8008), 1, "a");
}

#[test]
fn read_input_reflects_the_device_register() {
    let mut mem = build(
        r#"
use input;
use time;
fn main() {
    loop {
        var keys: u16 = read_input();
        poke(0x8000, keys);
        wait_tick();
    }
}
"#,
    );
    let mut machine = Machine::new(1);
    machine.tick(&mut mem, mmio::KEY_RIGHT, 10_000);
    let r16 = |a: u16| u16::from_le_bytes([mem[a as usize], mem[a as usize + 1]]);
    assert_eq!(r16(0x8000), mmio::KEY_RIGHT);
}

#[test]
fn wait_tick_blocks_for_exactly_one_tick() {
    // A budget of 0 per real tick means wait_tick() can only make progress
    // by the TICK register itself advancing, proving it isn't a busy-loop
    // that happens to finish within one CPU slice by luck.
    let mut mem = build("use time;\nfn main() { wait_tick(); poke(0x8000, 1); halt(); }");
    let mut machine = Machine::new(1);
    machine.tick(&mut mem, 0, 1_000); // ticks: 0 -> reads t=0, spins
    assert_eq!(machine.cpu.state, State::Running);
    machine.tick(&mut mem, 0, 1_000); // TICK becomes 1: loop exits
    assert_eq!(machine.cpu.state, State::Halted);
    assert_eq!(u16::from_le_bytes([mem[0x8000], mem[0x8001]]), 1);
}

#[test]
fn random_stays_within_bound_and_varies() {
    let (cpu, mem) = run(
        r#"
use random;
fn main() {
    poke(0x8000, random(10));
    poke(0x8002, random(10));
    poke(0x8004, random(10));
    halt();
}
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    let r16 = |a: u16| u16::from_le_bytes([mem[a as usize], mem[a as usize + 1]]);
    let (a, b, c) = (r16(0x8000), r16(0x8002), r16(0x8004));
    assert!(a < 10 && b < 10 && c < 10, "random(10) must stay in range: {a} {b} {c}");
    assert!(a != b || b != c, "successive calls should not all collide: {a} {b} {c}");
}

#[test]
fn user_definition_shadows_a_used_library() {
    // A user's own `clear_screen` must run instead of the library's — proof
    // that shadowing replaces, not just supplements, even when the library
    // providing the original name is `use`d.
    let (cpu, mem) = run(
        r#"
use screen;
fn clear_screen() {
    put_cell(0, 0, 'Z', RED);
}
fn main() {
    clear_screen();
    halt();
}
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cell(&mem, 0, 0), (b'Z', 9)); // RED = 9, not a blanked screen
}

#[test]
fn unreferenced_library_functions_are_not_emitted() {
    let asm = s16c::compile("use screen;\nuse random;\nfn main() { halt(); }").unwrap();
    assert!(!asm.contains("F_clear_screen"), "unused library fn must be excluded");
    assert!(!asm.contains("F_random"), "unused library fn must be excluded");
    assert!(!asm.contains("G_RAND_STATE"), "unused library static must be excluded");

    let asm = s16c::compile("use screen;\nfn main() { clear_screen(); halt(); }").unwrap();
    assert!(asm.contains("F_clear_screen"), "referenced library fn must be included");
    assert!(asm.contains("F_put_cell"), "transitively-called library fn must be included");
    assert!(!asm.contains("F_random"), "still-unused library fn must stay excluded");
}

#[test]
fn calling_an_unused_library_fails_with_a_hint() {
    let e = s16c::compile("fn main() { put_cell(0, 0, 'X', 1); halt(); }").unwrap_err();
    assert!(e.msg.contains("found in library `screen`"), "{}", e.msg);
    assert!(e.msg.contains("use screen;"), "{}", e.msg);
}

#[test]
fn unused_library_symbol_is_invisible_even_by_bare_name() {
    // A bare (non-call) reference to a name that only exists in a
    // non-imported library is a plain unknown-name error, not the
    // "add use X;" hint — the hint is deliberately scoped to call position
    // (see imports.rs) so it can't misfire on a same-named local variable.
    let e = s16c::compile("fn main() { var x: u16 = GREEN; halt(); }").unwrap_err();
    assert!(e.msg.contains("unknown"), "{}", e.msg);
    assert!(!e.msg.contains("use screen"), "{}", e.msg);
}

#[test]
fn unknown_library_name_lists_the_real_ones() {
    let e = s16c::compile("use graphics;\nfn main() { halt(); }").unwrap_err();
    assert!(e.msg.contains("no such library `graphics`"), "{}", e.msg);
    for lib in ["screen", "input", "time", "random"] {
        assert!(e.msg.contains(lib), "{}: missing `{lib}`", e.msg);
    }
}

#[test]
fn duplicate_use_is_harmless() {
    let (cpu, _) = run("use screen;\nuse screen;\nfn main() { clear_screen(); halt(); }");
    assert_eq!(cpu.state, State::Halted);
}

#[test]
fn use_order_does_not_matter() {
    // `use` is an item like any other: order among items is irrelevant
    // (LANGUAGE.md §2), so a `use` after the functions that need it must
    // work exactly like one at the top of the file.
    let (cpu, _) = run(
        r#"
fn main() {
    put_cell(0, 0, '@', GREEN);
    halt();
}
use screen;
"#,
    );
    assert_eq!(cpu.state, State::Halted);
}

#[test]
fn multiple_libraries_together() {
    let (cpu, mem) = run(
        r#"
use screen;
use input;
use random;
fn main() {
    var keys: u16 = 8; // RIGHT
    if key_right(keys) {
        put_cell(random(40), 0, '*', CYAN);
    }
    halt();
}
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    let addr = 0xF000usize;
    // Something in row 0 got drawn; exact column depends on the seeded RNG.
    let drawn = (0..40).any(|c| mem[addr + c * 2] == b'*');
    assert!(drawn, "expected a '*' drawn somewhere in row 0");
}
