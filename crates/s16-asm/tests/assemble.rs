//! The assembler is pinned to specs/ASSEMBLY.md: its worked example must
//! assemble byte-for-byte to the hand-assembled image, and the result must
//! actually run on s16-core.

use s16_asm::assemble;
use s16_core::{Cpu, State, MEMORY_SIZE};

/// The worked example source from ASSEMBLY.md, verbatim.
const SUM_SOURCE: &str = r#"
; sum.s16 — R0 = sum of the 5 words at data
        .org 0x0000

start:  LDI  R1, #data      ; R1 = pointer to list
        LDI  R2, #5         ; R2 = count
        LDI  R0, #0         ; R0 = running sum
loop:   CMPI R2, #0
        BEQ  done
        LDW  R3, [R1]
        ADD  R0, R0, R3
        ADDI R1, R1, #2     ; next word
        SUBI R2, R2, #1
        JMP  loop
done:   HALT

data:   .word 10, 20, 30, 40, 50
"#;

/// The normative hand-assembled image from ASSEMBLY.md, byte for byte.
const SUM_IMAGE: &[u8] = &[
    0x2C, 0x00, 0x10, 0x11, 0x05, 0x00, 0x20, 0x11, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x02,
    0x41, 0x05, 0x00, 0x00, 0x52, 0x00, 0x00, 0x31, 0x12, 0x00, 0x30, 0x00, 0x20, 0x02, 0x00,
    0x11, 0x30, 0x01, 0x00, 0x22, 0x31, 0xF9, 0xFF, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x0A,
    0x00, 0x14, 0x00, 0x1E, 0x00, 0x28, 0x00, 0x32, 0x00,
];

fn run(image: &[u8]) -> Cpu {
    let mut mem = vec![0u8; MEMORY_SIZE];
    mem[..image.len()].copy_from_slice(image);
    let mem: &mut [u8; MEMORY_SIZE] = (&mut mem[..]).try_into().unwrap();
    let mut cpu = Cpu::new();
    cpu.run(mem, 10_000);
    cpu
}

#[test]
fn sum_assembles_to_spec_image() {
    assert_eq!(assemble(SUM_SOURCE).unwrap(), SUM_IMAGE);
}

#[test]
fn assembled_sum_runs_on_core() {
    let cpu = run(&assemble(SUM_SOURCE).unwrap());
    assert_eq!(cpu.state, State::Halted);
    assert_eq!(cpu.regs[0], 150);
}

#[test]
fn wait_assembles_and_stops_the_cpu() {
    let image = assemble("LDI R1, #1\nWAIT\nLDI R2, #2\nHALT").unwrap();
    assert_eq!(&image[4..8], &[0x00, 0x00, 0x00, 0x03], "WAIT encodes as opcode 0x03, all other fields zero");
    let cpu = run(&image);
    assert_eq!(cpu.state, State::Waiting);
    assert_eq!(cpu.regs[2], 0, "the LDI after WAIT must not have run");
}

#[test]
fn case_insensitive_and_whitespace_tolerant() {
    let upper = assemble("LDI R1, #7\nHALT").unwrap();
    let lower = assemble("  ldi   r1 , #7  ; comment\nhalt").unwrap();
    assert_eq!(upper, lower);
}

#[test]
fn directives_and_literals() {
    let image = assemble(
        r#"
        .org 0x0100
msg:    .ascii "Hi"
        .asciiz "!"
        .byte 1, -1, 0xFF
        .space 3
        .word 0xBEEF, msg
"#,
    )
    .unwrap();
    assert_eq!(image.len(), 0x100 + 14);
    assert_eq!(&image[..0x100], &vec![0u8; 0x100][..], "gap zero-filled");
    assert_eq!(&image[0x100..0x104], b"Hi!\0");
    assert_eq!(&image[0x104..0x107], &[1, 0xFF, 0xFF]);
    assert_eq!(&image[0x107..0x10A], &[0, 0, 0]);
    assert_eq!(&image[0x10A..0x10C], &[0xEF, 0xBE], ".word little-endian");
    assert_eq!(&image[0x10C..0x10E], &[0x00, 0x01], ".word label = 0x0100");
}

#[test]
fn char_immediates_and_negative_offsets() {
    // LDI with a char literal
    let image = assemble("LDI R1, #'A'\nHALT").unwrap();
    assert_eq!(&image[..4], &[0x41, 0x00, 0x10, 0x11]);

    // LDW with a negative offset: imm16 = 0xFFFE
    let image = assemble("LDW R2, [R1-2]\nHALT").unwrap();
    assert_eq!(&image[..4], &[0xFE, 0xFF, 0x21, 0x12]);

    // escaped char
    let image = assemble("LDI R1, #'\\n'\nHALT").unwrap();
    assert_eq!(image[0], b'\n');
}

#[test]
fn rsp_wsp_encode() {
    let image = assemble("RSP R3\nWSP R4\nHALT").unwrap();
    assert_eq!(&image[..4], &[0x00, 0x00, 0x30, 0x62], "RSP rd in the rd field");
    assert_eq!(&image[4..8], &[0x00, 0x00, 0x04, 0x63], "WSP rs in the rs1 field");
}

#[test]
fn raw_numeric_branch_offset() {
    // "JMP #-1" = branch to self, same as a label on the line
    let by_number = assemble("JMP #-1").unwrap();
    let by_label = assemble("self: JMP self").unwrap();
    assert_eq!(by_number, by_label);
}

#[test]
fn semicolon_inside_literals_is_not_a_comment() {
    let image = assemble(".ascii \"a;b\"").unwrap();
    assert_eq!(image, b"a;b");
    let image = assemble("LDI R1, #';'\nHALT").unwrap();
    assert_eq!(image[0], b';');
}

#[test]
fn errors_carry_line_numbers() {
    let cases = [
        ("NOP\nBOGUS R1", 2, "unknown mnemonic"),
        ("JMP nowhere", 1, "undefined label"),
        ("x: NOP\nx: NOP", 2, "duplicate label"),
        ("LDI R16, #0", 1, "expected a register"),
        ("LDI R1, 5", 1, "immediates start with '#'"),
        ("LDI R1, #70000", 1, "does not fit in 16 bits"),
        (".org 0x2\nNOP", 2, "not 4-byte aligned"),
        ("ADD R1, R2", 1, "expects 3 operand(s)"),
        (".byte 300", 1, "not a valid byte value"),
    ];
    for (src, line, needle) in cases {
        let e = assemble(src).expect_err(src);
        assert_eq!(e.line, line, "line number for {src:?}");
        assert!(e.msg.contains(needle), "{src:?}: got `{}`", e.msg);
    }
}
