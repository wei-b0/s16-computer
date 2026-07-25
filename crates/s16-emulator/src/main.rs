//! Local S16 emulator.
//!
//! Loads a flat binary image at 0x0000 and runs it to completion, printing
//! the final CPU state. Framebuffer rendering and INPUT/RNG/TICK device
//! emulation come next; see specs/MEMORY_MAP.md.

use s16_core::{Cpu, State, MEMORY_SIZE};
use std::process::ExitCode;

const MAX_STEPS: u64 = 100_000_000;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: s16-emulator <image.bin>");
        eprintln!("loads the flat binary at 0x0000 and runs until the CPU stops");
        return ExitCode::FAILURE;
    };

    let image = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("s16-emulator: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    if image.len() > MEMORY_SIZE {
        eprintln!(
            "s16-emulator: image is {} bytes, exceeds {} KB of memory",
            image.len(),
            MEMORY_SIZE / 1024
        );
        return ExitCode::FAILURE;
    }

    let mut mem = vec![0u8; MEMORY_SIZE];
    mem[..image.len()].copy_from_slice(&image);
    let mem: &mut [u8; MEMORY_SIZE] = (&mut mem[..]).try_into().unwrap();

    let mut cpu = Cpu::new();
    let steps = cpu.run(mem, MAX_STEPS);

    match cpu.state {
        State::Halted => println!("halted after {steps} instructions"),
        State::DebugHalt => println!("hit BRK after {steps} instructions"),
        State::Faulted(fault) => println!(
            "FAULT {} ({fault:?}) at PC={:#06X} after {steps} instructions",
            fault.code(),
            cpu.pc
        ),
        State::Running => println!("still running after {steps} instructions (step budget hit)"),
    }

    println!("PC={:#06X}  SP={:#06X}  FLAGS={:#06b}", cpu.pc, cpu.sp, cpu.flags);
    for row in 0..4 {
        let line: Vec<String> = (0..4)
            .map(|col| {
                let n = row * 4 + col;
                format!("R{n:<2}={:#06X}", cpu.regs[n])
            })
            .collect();
        println!("{}", line.join("  "));
    }

    match cpu.state {
        State::Halted | State::DebugHalt => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
