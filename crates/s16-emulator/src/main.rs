//! Local S16 emulator.
//!
//! Two modes:
//! - batch (default): pure CPU — run the image to completion, print state.
//! - `--tui`: the full machine — framebuffer rendered to the terminal,
//!   keyboard feeding INPUT, RNG/TICK live (specs/MEMORY_MAP.md).

mod tui;

use s16_core::{Cpu, State, MEMORY_SIZE};
use std::io::IsTerminal;
use std::process::ExitCode;
use std::time::Instant;

const MAX_STEPS: u64 = 100_000_000;
const DEFAULT_SEED: u16 = 0xACE1;
const DEFAULT_BUDGET: u32 = 10_000;

struct Args {
    image: String,
    tui: bool,
    seed: u16,
    budget: u32,
}

fn usage() -> ExitCode {
    eprintln!("usage: s16-emulator [--tui] [--seed N] [--budget N] <image.bin>");
    eprintln!("  batch (default): run the CPU to completion and print final state");
    eprintln!("  --tui:           interactive machine with display and keyboard");
    eprintln!("  --seed N:        RNG seed for the machine (default 0xACE1)");
    eprintln!("  --budget N:      instructions per tick in --tui mode (default 10000)");
    ExitCode::FAILURE
}

fn parse_num(s: &str) -> Option<u32> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

fn parse_args() -> Result<Args, ExitCode> {
    let mut args = Args {
        image: String::new(),
        tui: false,
        seed: DEFAULT_SEED,
        budget: DEFAULT_BUDGET,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--tui" => args.tui = true,
            "--seed" | "--budget" => {
                let value = iter.next().and_then(|v| parse_num(&v));
                match (arg.as_str(), value) {
                    ("--seed", Some(v)) if v <= 0xFFFF => args.seed = v as u16,
                    ("--budget", Some(v)) if v > 0 => args.budget = v,
                    _ => {
                        eprintln!("s16-emulator: bad value for {arg}");
                        return Err(usage());
                    }
                }
            }
            _ if args.image.is_empty() && !arg.starts_with('-') => args.image = arg,
            _ => {
                eprintln!("s16-emulator: unexpected argument `{arg}`");
                return Err(usage());
            }
        }
    }
    if args.image.is_empty() {
        return Err(usage());
    }
    Ok(args)
}

fn print_registers(cpu: &Cpu) {
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
}

fn exit_code(state: State) -> ExitCode {
    match state {
        State::Faulted(_) => ExitCode::FAILURE,
        _ => ExitCode::SUCCESS,
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(code) => return code,
    };

    let image = match std::fs::read(&args.image) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("s16-emulator: cannot read {}: {e}", args.image);
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

    if args.tui { run_tui(&args, &image) } else { run_batch(&image) }
}

fn run_tui(args: &Args, image: &[u8]) -> ExitCode {
    if !std::io::stdout().is_terminal() {
        eprintln!("s16-emulator: --tui needs a terminal (stdout is not a tty)");
        return ExitCode::FAILURE;
    }
    match tui::run(image, args.seed, args.budget) {
        Ok((machine, executed)) => {
            println!(
                "{} after {} ticks, {executed} instructions",
                match machine.cpu.state {
                    State::Running => "interrupted while running",
                    State::Waiting => "waiting for input",
                    State::Halted => "halted",
                    State::DebugHalt => "stopped at BRK",
                    State::Faulted(_) => "FAULTED",
                },
                machine.ticks
            );
            print_registers(&machine.cpu);
            exit_code(machine.cpu.state)
        }
        Err(e) => {
            eprintln!("s16-emulator: terminal error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_batch(image: &[u8]) -> ExitCode {
    let mut mem = vec![0u8; MEMORY_SIZE];
    mem[..image.len()].copy_from_slice(image);
    let mem: &mut [u8; MEMORY_SIZE] = (&mut mem[..]).try_into().unwrap();

    let mut cpu = Cpu::new();
    let start = Instant::now();
    let steps = cpu.run(mem, MAX_STEPS);
    let elapsed = start.elapsed();

    match cpu.state {
        State::Halted => println!("halted after {steps} instructions"),
        State::DebugHalt => println!("hit BRK after {steps} instructions"),
        State::Waiting => {
            println!("waiting for input after {steps} instructions (nothing will wake it in batch mode)")
        }
        State::Faulted(fault) => println!(
            "FAULT {} ({fault:?}) at PC={:#06X} after {steps} instructions",
            fault.code(),
            cpu.pc
        ),
        State::Running => println!("still running after {steps} instructions (step budget hit)"),
    }
    let ips = steps as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
    println!("{steps} instructions in {:.3} ms (~{ips:.0} IPS)", elapsed.as_secs_f64() * 1000.0);
    print_debug_output(mem);
    print_registers(&cpu);

    match cpu.state {
        State::Halted | State::DebugHalt => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// Batch mode still does not tick or service INPUT/RNG/TICK/STATUS at all
/// (D025) — this is one narrow, deliberate exception: a one-shot final read
/// of whatever the program left in the debug-console buffer, straight out
/// of `mem`, no `Machine` involved. It's what lets a simple `debug::print*`
/// example print its result without needing `--tui`.
fn print_debug_output(mem: &[u8; MEMORY_SIZE]) {
    let len = u16::from_le_bytes([
        mem[s16_machine::mmio::DEBUG_LEN as usize],
        mem[s16_machine::mmio::DEBUG_LEN as usize + 1],
    ]) as usize;
    let len = len.min(s16_machine::mmio::DEBUG_BUF_CAP);
    if len == 0 {
        return;
    }
    let base = s16_machine::mmio::DEBUG_BUF as usize;
    let text = String::from_utf8_lossy(&mem[base..base + len]);
    println!("debug output:");
    print!("{text}");
    if !text.ends_with('\n') {
        println!();
    }
}
