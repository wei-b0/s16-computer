//! The standard libraries (crates/s16c/src/libs/*.s16c), exercised end to
//! end: compile -> assemble -> run/tick on the real CPU/machine. Covers
//! behavior, module-qualified calls, dead-code exclusion, and the `use`
//! mechanism itself (specs/COMPILER.md "Standard Library Merge").

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
fn put_and_get_round_trip() {
    let (cpu, mem) = run(
        r#"
use screen;
fn main() {
    screen::put(5, 3, 'X', screen::GREEN);
    var back: u16 = screen::get(5, 3);
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
fn clear_blanks_every_cell() {
    let (cpu, mem) = run(
        r#"
use screen;
fn main() {
    screen::put(0, 0, '#', screen::WHITE);
    screen::put(39, 24, '#', screen::WHITE);
    screen::clear();
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
fn print_draws_a_string_left_to_right() {
    let (cpu, mem) = run(
        r#"
use screen;
fn main() {
    screen::print(0, 0, "HI", screen::WHITE);
    halt();
}
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cell(&mem, 0, 0), (b'H', 15));
    assert_eq!(cell(&mem, 1, 0), (b'I', 15));
}

#[test]
fn key_helpers_decode_the_input_bitmask() {
    let (cpu, mem) = run(
        r#"
use input;
fn main() {
    var keys: u16 = 21;   // UP | LEFT | A  (bits 0, 2, 4 -> 1+4+16)
    if input::key_up(keys)    { poke(0x8000, 1); }
    if input::key_down(keys)  { poke(0x8002, 1); }
    if input::key_left(keys)  { poke(0x8004, 1); }
    if input::key_right(keys) { poke(0x8006, 1); }
    if input::key_a(keys)     { poke(0x8008, 1); }
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
fn read_reflects_the_device_register() {
    let mut mem = build(
        r#"
use input;
use time;
fn main() {
    loop {
        var keys: u16 = input::read();
        poke(0x8000, keys);
        time::wait_next();
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
fn wait_next_blocks_for_exactly_one_tick() {
    // wait_next() parks the CPU (WAITTICK) rather than busy-polling TICK, so
    // it costs ~0 instructions while idle and always resumes on the very
    // next tick, regardless of how small the budget is.
    let mut mem = build("use time;\nfn main() { time::wait_next(); poke(0x8000, 1); halt(); }");
    let mut machine = Machine::new(1);
    machine.tick(&mut mem, 0, 1_000); // runs up to WAITTICK, parks
    assert_eq!(machine.cpu.state, State::WaitingTick);
    assert_eq!(u16::from_le_bytes([mem[0x8000], mem[0x8001]]), 0, "not written yet");
    machine.tick(&mut mem, 0, 1_000); // next tick wakes it unconditionally
    assert_eq!(machine.cpu.state, State::Halted);
    assert_eq!(u16::from_le_bytes([mem[0x8000], mem[0x8001]]), 1);
}

#[test]
fn wait_key_blocks_until_a_key_is_pressed() {
    let mut mem = build("use input;\nfn main() { poke(0x8000, input::wait_key()); halt(); }");
    let mut machine = Machine::new(1);

    machine.tick(&mut mem, 0, 1_000); // no key: parks inside wait_key()
    assert_eq!(machine.cpu.state, State::Waiting);
    assert_eq!(u16::from_le_bytes([mem[0x8000], mem[0x8001]]), 0, "not written yet");

    machine.tick(&mut mem, mmio::KEY_A, 1_000); // wakes, re-reads, returns, halts
    assert_eq!(machine.cpu.state, State::Halted);
    assert_eq!(u16::from_le_bytes([mem[0x8000], mem[0x8001]]), mmio::KEY_A);
}

#[test]
fn rng_next_stays_within_bound_and_varies() {
    let (cpu, mem) = run(
        r#"
use rng;
fn main() {
    poke(0x8000, rng::next(10));
    poke(0x8002, rng::next(10));
    poke(0x8004, rng::next(10));
    halt();
}
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    let r16 = |a: u16| u16::from_le_bytes([mem[a as usize], mem[a as usize + 1]]);
    let (a, b, c) = (r16(0x8000), r16(0x8002), r16(0x8004));
    assert!(a < 10 && b < 10 && c < 10, "rng::next(10) must stay in range: {a} {b} {c}");
    assert!(a != b || b != c, "successive calls should not all collide: {a} {b} {c}");
}

#[test]
fn debug_print_and_print_u16_write_the_debug_console() {
    let mut mem = build(
        r#"
use debug;
fn main() {
    debug::print("Score: ");
    debug::print_u16(150);
    halt();
}
"#,
    );
    let mut machine = Machine::new(1);
    machine.tick(&mut mem, 0, 10_000);
    assert_eq!(machine.debug_output(), b"Score: 150");
}

#[test]
fn debug_println_appends_a_newline() {
    let mut mem = build("use debug;\nfn main() { debug::println(\"hi\"); halt(); }");
    let mut machine = Machine::new(1);
    machine.tick(&mut mem, 0, 10_000);
    assert_eq!(machine.debug_output(), b"hi\n");
}

#[test]
fn debug_print_i16_handles_sign_including_min_value() {
    let mut mem = build(
        r#"
use debug;
fn main() {
    debug::print_i16(-5);
    debug::print(" ");
    debug::print_i16(5);
    debug::print(" ");
    debug::print_i16(-32768);
    halt();
}
"#,
    );
    let mut machine = Machine::new(1);
    machine.tick(&mut mem, 0, 10_000);
    assert_eq!(machine.debug_output(), b"-5 5 -32768");
}

#[test]
fn debug_print_hex_writes_four_uppercase_digits() {
    let mut mem = build("use debug;\nfn main() { debug::print_hex(0xBEEF); halt(); }");
    let mut machine = Machine::new(1);
    machine.tick(&mut mem, 0, 10_000);
    assert_eq!(machine.debug_output(), b"0xBEEF");
}

#[test]
fn unreferenced_library_functions_are_not_emitted() {
    let asm = s16c::compile("use screen;\nuse rng;\nfn main() { halt(); }").unwrap();
    assert!(!asm.contains("F_screen__clear"), "unused library fn must be excluded");
    assert!(!asm.contains("F_rng__next"), "unused library fn must be excluded");
    assert!(!asm.contains("G_rng__RAND_STATE"), "unused library static must be excluded");

    let asm = s16c::compile("use screen;\nfn main() { screen::clear(); halt(); }").unwrap();
    assert!(asm.contains("F_screen__clear"), "referenced library fn must be included");
    assert!(asm.contains("F_screen__put"), "transitively-called library fn must be included");
    assert!(!asm.contains("F_rng__next"), "still-unused library fn must stay excluded");
}

#[test]
fn calling_an_unused_library_fails_with_a_hint() {
    let e = s16c::compile("fn main() { screen::put(0, 0, 'X', 1); halt(); }").unwrap_err();
    assert!(e.msg.contains("found in library `screen`"), "{}", e.msg);
    assert!(e.msg.contains("use screen;"), "{}", e.msg);
}

#[test]
fn bare_unqualified_call_never_resolves_against_a_library() {
    // There's no bare-name fallback after module-qualification (D023): a
    // call to unqualified `put(...)` is a plain unknown-function error, not
    // the "add use screen;" hint — that hint only fires for a *qualified*
    // call to a real library that wasn't `use`d.
    let e = s16c::compile("use screen;\nfn main() { put(0, 0, 'X', 1); halt(); }").unwrap_err();
    assert!(e.msg.contains("unknown function `put`"), "{}", e.msg);
    assert!(!e.msg.contains("found in library"), "{}", e.msg);
}

#[test]
fn unused_library_symbol_is_invisible_even_by_bare_name() {
    // A bare (non-call, non-qualified) reference to a name that only exists
    // as a qualified library export is a plain unknown-name error, not the
    // "add use X;" hint — the hint is scoped to qualified *calls* only.
    let e = s16c::compile("fn main() { var x: u16 = GREEN; halt(); }").unwrap_err();
    assert!(e.msg.contains("unknown"), "{}", e.msg);
    assert!(!e.msg.contains("use screen"), "{}", e.msg);
}

#[test]
fn unknown_library_name_lists_the_real_ones() {
    let e = s16c::compile("use graphics;\nfn main() { halt(); }").unwrap_err();
    assert!(e.msg.contains("no such library `graphics`"), "{}", e.msg);
    for lib in ["screen", "input", "time", "rng", "debug"] {
        assert!(e.msg.contains(lib), "{}: missing `{lib}`", e.msg);
    }
}

#[test]
fn duplicate_use_is_harmless() {
    let (cpu, _) = run("use screen;\nuse screen;\nfn main() { screen::clear(); halt(); }");
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
    screen::put(0, 0, '@', screen::GREEN);
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
use rng;
fn main() {
    var keys: u16 = 8; // RIGHT
    if input::key_right(keys) {
        screen::put(rng::next(40), 0, '*', screen::CYAN);
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

#[test]
fn two_libraries_may_share_an_internal_helper_name() {
    // screen and debug each have their own internal `write_byte`-shaped or
    // otherwise-named helpers; qualifying by library (D023) means this
    // can't collide, unlike the old single flat cross-library namespace.
    // Exercised indirectly: both libraries' *public* surface works together
    // in one program without any special handling.
    let (cpu, mem) = run(
        r#"
use screen;
use debug;
fn main() {
    screen::put(0, 0, '@', screen::GREEN);
    debug::print_u16(42);
    halt();
}
"#,
    );
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cell(&mem, 0, 0), (b'@', 10));
}
