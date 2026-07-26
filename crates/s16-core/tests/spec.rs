//! Tests against the spec in `specs/`. The `sum` image is the normative
//! hand-assembled test vector from ASSEMBLY.md — the assembler and the
//! on-chain host must agree with it byte for byte.

use s16_core::{opcode::*, Cpu, Fault, State, FLAG_C, FLAG_N, FLAG_V, FLAG_Z, MEMORY_SIZE};

fn r(op: u8, rd: u16, rs1: u16, rs2: u16) -> u32 {
    (op as u32) << 24 | (rd as u32) << 20 | (rs1 as u32) << 16 | (rs2 as u32) << 12
}

fn i(op: u8, rd: u16, rs1: u16, imm: u16) -> u32 {
    (op as u32) << 24 | (rd as u32) << 20 | (rs1 as u32) << 16 | imm as u32
}

fn j(op: u8, offset: i16) -> u32 {
    (op as u32) << 24 | offset as u16 as u32
}

fn memory(words: &[u32]) -> Box<[u8; MEMORY_SIZE]> {
    let mut mem = vec![0u8; MEMORY_SIZE];
    for (n, w) in words.iter().enumerate() {
        mem[n * 4..n * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    mem.into_boxed_slice().try_into().unwrap()
}

/// Run a fresh CPU on the given program until it stops.
fn run(words: &[u32]) -> (Cpu, Box<[u8; MEMORY_SIZE]>) {
    let mut mem = memory(words);
    let mut cpu = Cpu::new();
    cpu.run(&mut mem, 10_000);
    (cpu, mem)
}

// --- The ASSEMBLY.md test vector -------------------------------------------

/// Raw memory image from ASSEMBLY.md "Worked Example", byte for byte.
const SUM_IMAGE: &[u8] = &[
    0x2C, 0x00, 0x10, 0x11, 0x05, 0x00, 0x20, 0x11, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x02,
    0x41, 0x05, 0x00, 0x00, 0x52, 0x00, 0x00, 0x31, 0x12, 0x00, 0x30, 0x00, 0x20, 0x02, 0x00,
    0x11, 0x30, 0x01, 0x00, 0x22, 0x31, 0xF9, 0xFF, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x0A,
    0x00, 0x14, 0x00, 0x1E, 0x00, 0x28, 0x00, 0x32, 0x00,
];

#[test]
fn assembly_md_sum_vector() {
    let mut mem = vec![0u8; MEMORY_SIZE];
    mem[..SUM_IMAGE.len()].copy_from_slice(SUM_IMAGE);
    let mut mem: Box<[u8; MEMORY_SIZE]> = mem.into_boxed_slice().try_into().unwrap();

    let mut cpu = Cpu::new();
    let steps = cpu.run(&mut mem, 10_000);

    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cpu.regs[0], 150, "sum of 10+20+30+40+50");
    // 3 setup + 5 iterations of 7 + final CMPI/BEQ + HALT
    assert_eq!(steps, 41);
}

/// The same program built with the encoding helpers must reproduce the
/// hand-assembled image exactly — this pins the encoders to the spec.
#[test]
fn assembly_md_sum_encoding() {
    let words = [
        i(LDI, 1, 0, 0x002C),
        i(LDI, 2, 0, 5),
        i(LDI, 0, 0, 0),
        i(CMPI, 0, 2, 0),
        j(BEQ, 5),
        i(LDW, 3, 1, 0),
        r(ADD, 0, 0, 3),
        i(ADDI, 1, 1, 2),
        i(SUBI, 2, 2, 1),
        j(JMP, -7),
        r(HALT, 0, 0, 0),
    ];
    let mut image: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    for data in [10u16, 20, 30, 40, 50] {
        image.extend_from_slice(&data.to_le_bytes());
    }
    assert_eq!(image, SUM_IMAGE);
}

// --- ALU and data movement --------------------------------------------------

#[test]
fn alu_wrapping_and_logic() {
    let (cpu, _) = run(&[
        i(LDI, 1, 0, 0xFFFF),
        i(ADDI, 2, 1, 1),      // wraps to 0
        i(LDI, 3, 0, 0x00F0),
        r(NOT, 4, 3, 0),       // 0xFF0F
        i(LDI, 5, 0, 7),
        i(LDI, 6, 0, 3),
        r(MUL, 7, 5, 6),       // 21
        r(DIV, 8, 5, 6),       // 2
        r(MOD, 9, 5, 6),       // 1
        r(HALT, 0, 0, 0),
    ]);
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cpu.regs[2], 0);
    assert_eq!(cpu.regs[4], 0xFF0F);
    assert_eq!(cpu.regs[7], 21);
    assert_eq!(cpu.regs[8], 2);
    assert_eq!(cpu.regs[9], 1);
}

#[test]
fn shifts_saturate_at_16() {
    let (cpu, _) = run(&[
        i(LDI, 1, 0, 0x8001),
        i(SHLI, 2, 1, 20), // 0
        i(SHRI, 3, 1, 20), // 0
        i(SARI, 4, 1, 20), // sign fill -> 0xFFFF
        i(SARI, 5, 1, 1),  // 0xC000
        r(HALT, 0, 0, 0),
    ]);
    assert_eq!(cpu.regs[2], 0);
    assert_eq!(cpu.regs[3], 0);
    assert_eq!(cpu.regs[4], 0xFFFF);
    assert_eq!(cpu.regs[5], 0xC000);
}

#[test]
fn byte_access_zero_extends() {
    let (cpu, mem) = run(&[
        i(LDI, 1, 0, 0x1000),
        i(LDI, 2, 0, 0xABCD),
        i(STB, 2, 1, 0),   // writes low byte 0xCD
        i(LDB, 3, 1, 0),   // reads back 0x00CD
        i(STW, 2, 1, 2),
        i(LDB, 4, 1, 3),   // high byte of the word: 0xAB
        r(HALT, 0, 0, 0),
    ]);
    assert_eq!(cpu.regs[3], 0x00CD);
    assert_eq!(cpu.regs[4], 0x00AB);
    assert_eq!(mem[0x1000], 0xCD);
    assert_eq!(mem[0x1001], 0x00, "STB must not touch the next byte");
}

#[test]
fn negative_load_offset() {
    let (cpu, _) = run(&[
        i(LDI, 1, 0, 0x2002),
        i(LDI, 2, 0, 0xBEEF),
        i(STW, 2, 1, 0xFFFE), // [R1-2] = 0x2000
        i(LDW, 3, 1, 0xFFFE),
        r(HALT, 0, 0, 0),
    ]);
    assert_eq!(cpu.regs[3], 0xBEEF);
}

// --- Flags and branches ------------------------------------------------------

#[test]
fn cmp_flag_cases() {
    // (a, b, expected flags)
    let cases = [
        (5u16, 5u16, FLAG_Z),
        (3, 5, FLAG_N | FLAG_C), // 3 < 5 in both signed and unsigned views
        (5, 3, 0),
        (0x8000, 1, FLAG_V),     // -32768 - 1 overflows signed, no borrow
        (0xFFFF, 1, FLAG_N),     // -1 - 1 = -2: negative, no borrow, no overflow
    ];
    for (a, b, expected) in cases {
        let (cpu, _) = run(&[
            i(LDI, 1, 0, a),
            i(CMPI, 0, 1, b),
            r(HALT, 0, 0, 0),
        ]);
        assert_eq!(cpu.flags, expected, "CMP {a:#06X}, {b:#06X}");
    }
}

#[test]
fn signed_vs_unsigned_branches() {
    // 0xFFFF is -1 signed but 65535 unsigned: BLT taken, BLTU not.
    let (cpu, _) = run(&[
        i(LDI, 1, 0, 0xFFFF),
        i(CMPI, 0, 1, 1),      // -1 < 1 signed; 65535 > 1 unsigned
        j(BLT, 1),             // -> set R2
        r(HALT, 0, 0, 0),
        i(LDI, 2, 0, 1),       // BLT target
        i(CMPI, 0, 1, 1),
        j(BLTU, 1),            // must NOT branch (to HALT below skipping R3)
        i(LDI, 3, 0, 1),
        r(HALT, 0, 0, 0),
    ]);
    assert_eq!(cpu.regs[2], 1, "BLT (signed) taken");
    assert_eq!(cpu.regs[3], 1, "BLTU (unsigned) not taken");
}

#[test]
fn only_cmp_writes_flags() {
    let (cpu, _) = run(&[
        i(LDI, 1, 0, 5),
        i(CMPI, 0, 1, 5),      // Z set
        i(SUBI, 1, 1, 5),      // result 0, but must not touch FLAGS
        i(ADDI, 1, 1, 9),
        r(HALT, 0, 0, 0),
    ]);
    assert_eq!(cpu.flags, FLAG_Z);
}

// --- Stack, calls, jumps -----------------------------------------------------

#[test]
fn push_pop_call_ret() {
    let (cpu, mem) = run(&[
        i(LDI, 1, 0, 0x1234),
        r(PUSH, 0, 1, 0),
        j(CALL, 2),            // call `double` at word 5
        r(POP, 3, 0, 0),       // 0x1234 still below the (consumed) frame
        r(HALT, 0, 0, 0),
        // double: R2 = R1 * 2
        r(ADD, 2, 1, 1),       // word 5
        r(RET, 0, 0, 0),
    ]);
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cpu.regs[2], 0x2468);
    assert_eq!(cpu.regs[3], 0x1234);
    assert_eq!(cpu.sp, s16_core::SP_RESET, "stack balanced");
    // return address 0x000C was pushed at 0xEFFC before RET consumed it
    assert_eq!(u16::from_le_bytes([mem[0xEFFC], mem[0xEFFD]]), 0x000C);
}

#[test]
fn rsp_wsp_build_a_stack_frame() {
    let (cpu, _) = run(&[
        r(RSP, 1, 0, 0),      // R1 = SP = 0xF000
        i(SUBI, 2, 1, 8),     // reserve 8 bytes: R2 = 0xEFF8
        r(WSP, 0, 2, 0),      // SP = R2
        i(LDI, 5, 0, 0xBEEF),
        i(STW, 5, 2, 2),      // a "local" at [R2+2]
        i(LDW, 6, 2, 2),
        r(WSP, 0, 1, 0),      // restore SP
        r(RSP, 7, 0, 0),
        r(HALT, 0, 0, 0),
    ]);
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cpu.regs[1], 0xF000, "RSP reads the reset SP");
    assert_eq!(cpu.regs[6], 0xBEEF, "local written and read through the frame");
    assert_eq!(cpu.regs[7], 0xF000, "WSP restored SP");
    assert_eq!(cpu.sp, 0xF000);
}

#[test]
fn wsp_faults_on_odd_address() {
    let (cpu, _) = run(&[i(LDI, 1, 0, 0x1001), r(WSP, 0, 1, 0)]);
    assert_eq!(cpu.state, State::Faulted(Fault::UnalignedAccess));
    assert_eq!(cpu.sp, 0xF000, "SP untouched by the faulting WSP");
}

#[test]
fn jr_jumps_absolute() {
    let (cpu, _) = run(&[
        i(LDI, 1, 0, 0x000C), // address of word 3
        r(JR, 0, 1, 0),
        i(LDI, 2, 0, 0xDEAD), // skipped
        r(HALT, 0, 0, 0),     // word 3
    ]);
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cpu.regs[2], 0);
}

// --- Stopping and faults -----------------------------------------------------

#[test]
fn halt_brk_pc_semantics() {
    let (cpu, _) = run(&[r(NOP, 0, 0, 0), r(HALT, 0, 0, 0)]);
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cpu.pc, 0x0008, "PC points past HALT");

    let (cpu, _) = run(&[r(BRK, 0, 0, 0)]);
    assert_eq!(cpu.state, State::DebugHalt);
    assert_eq!(cpu.pc, 0x0004, "PC points past BRK");
}

#[test]
fn faults() {
    // Illegal opcode
    let (cpu, _) = run(&[r(0xEE, 0, 0, 0)]);
    assert_eq!(cpu.state, State::Faulted(Fault::IllegalOpcode));
    assert_eq!(cpu.pc, 0x0000, "PC stays at faulting instruction");

    // Unaligned LDW
    let (cpu, _) = run(&[i(LDI, 1, 0, 0x1001), i(LDW, 2, 1, 0)]);
    assert_eq!(cpu.state, State::Faulted(Fault::UnalignedAccess));
    assert_eq!(cpu.pc, 0x0004);

    // Unaligned JR target
    let (cpu, _) = run(&[i(LDI, 1, 0, 0x0102), r(JR, 0, 1, 0)]);
    assert_eq!(cpu.state, State::Faulted(Fault::UnalignedAccess));

    // Divide and modulo by zero
    let (cpu, _) = run(&[i(LDI, 1, 0, 9), r(DIV, 2, 1, 3)]);
    assert_eq!(cpu.state, State::Faulted(Fault::DivByZero));
    let (cpu, _) = run(&[i(LDI, 1, 0, 9), r(MOD, 2, 1, 3)]);
    assert_eq!(cpu.state, State::Faulted(Fault::DivByZero));
}

#[test]
fn stopped_cpu_does_not_step() {
    let mut mem = memory(&[r(HALT, 0, 0, 0), i(LDI, 1, 0, 1)]);
    let mut cpu = Cpu::new();
    cpu.run(&mut mem, 100);
    let after_halt = cpu.clone();
    cpu.step(&mut mem);
    assert_eq!(cpu.pc, after_halt.pc);
    assert_eq!(cpu.regs, after_halt.regs);
    assert_eq!(cpu.state, State::Halted);
}

#[test]
fn wait_stops_the_run_loop_immediately() {
    let mut mem = memory(&[
        i(LDI, 1, 0, 1),
        r(WAIT, 0, 0, 0),
        i(LDI, 2, 0, 0xDEAD), // never reached: the run loop stops at WAIT
    ]);
    let mut cpu = Cpu::new();
    let steps = cpu.run(&mut mem, 100);
    assert_eq!(steps, 2, "run() must not consume the rest of the budget past WAIT");
    assert_eq!(cpu.state, State::Waiting);
    assert_eq!(cpu.pc, 0x0008, "PC points past WAIT");
    assert_eq!(cpu.regs[2], 0);
}

#[test]
fn waiting_cpu_does_not_step() {
    let mut mem = memory(&[r(WAIT, 0, 0, 0), i(LDI, 1, 0, 1)]);
    let mut cpu = Cpu::new();
    cpu.run(&mut mem, 100);
    let after_wait = cpu.clone();
    cpu.step(&mut mem);
    assert_eq!(cpu.pc, after_wait.pc);
    assert_eq!(cpu.regs, after_wait.regs);
    assert_eq!(cpu.state, State::Waiting);
}
