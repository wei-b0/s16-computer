//! The S16 machine: the tick loop and MMIO devices around the CPU.
//!
//! `s16-core` executes instructions; this crate makes those executions a
//! *running computer*. Each tick the machine services the memory-mapped
//! devices (INPUT, RNG, TICK), runs a budget of CPU instructions, and updates
//! STATUS — the model described in specs/MEMORY_MAP.md ("Machine Tick").
//!
//! Like the core, this crate is `no_std`, dependency-free, and deterministic:
//! the same seed, inputs, and budgets produce the same machine state on every
//! host. The emulator maps a tick to a rendered frame; the Solana host maps
//! it to a transaction. Host-specific concerns (rendering, keyboards, clocks)
//! stay outside — hosts feed `tick()` an input bitmask and draw the
//! framebuffer region however they like.

#![no_std]

use s16_core::{Cpu, State, MEMORY_SIZE};

/// MMIO addresses and layout constants from specs/MEMORY_MAP.md.
pub mod mmio {
    /// Framebuffer base: 40x25 cells, 2 bytes each (char, attribute).
    pub const FRAMEBUFFER: u16 = 0xF000;
    pub const FB_COLS: u16 = 40;
    pub const FB_ROWS: u16 = 25;

    /// Byte address of a framebuffer cell.
    pub fn cell_addr(row: u16, col: u16) -> u16 {
        FRAMEBUFFER + (row * FB_COLS + col) * 2
    }

    /// Key-state bitmask register (R).
    pub const INPUT: u16 = 0xFF00;
    /// Pseudorandom value, advanced once per tick (R).
    pub const RNG: u16 = 0xFF02;
    /// Machine tick counter (R).
    pub const TICK: u16 = 0xFF04;
    /// Execution status: low byte = state, high byte = fault code (R).
    pub const STATUS: u16 = 0xFF06;

    /// INPUT register bits.
    pub const KEY_UP: u16 = 1 << 0;
    pub const KEY_DOWN: u16 = 1 << 1;
    pub const KEY_LEFT: u16 = 1 << 2;
    pub const KEY_RIGHT: u16 = 1 << 3;
    pub const KEY_A: u16 = 1 << 4;
    pub const KEY_B: u16 = 1 << 5;
    pub const KEY_START: u16 = 1 << 6;
    pub const KEY_SELECT: u16 = 1 << 7;
}

/// The 16-bit xorshift generator pinned in specs/MEMORY_MAP.md.
/// State is never zero, so the sequence has full period (65535).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Xorshift16(u16);

impl Xorshift16 {
    /// A zero seed is treated as 1 (xorshift cannot leave state zero).
    pub fn new(seed: u16) -> Self {
        Xorshift16(if seed == 0 { 1 } else { seed })
    }

    /// Advance the state and return the new value.
    pub fn advance(&mut self) -> u16 {
        let mut x = self.0;
        x ^= x << 7;
        x ^= x >> 9;
        x ^= x << 8;
        self.0 = x;
        x
    }
}

/// The machine: CPU plus device state. Memory is borrowed per tick, exactly
/// like `Cpu::step`, so hosts keep it wherever they need.
#[derive(Debug, Clone)]
pub struct Machine {
    pub cpu: Cpu,
    pub ticks: u16,
    rng: Xorshift16,
}

impl Machine {
    pub fn new(rng_seed: u16) -> Self {
        Machine {
            cpu: Cpu::new(),
            ticks: 0,
            rng: Xorshift16::new(rng_seed),
        }
    }

    /// Run one machine tick (specs/MEMORY_MAP.md "Machine Tick"):
    ///
    /// 1. write `input` to INPUT, advance the RNG into RNG, write the tick
    ///    counter to TICK;
    /// 2. execute up to `budget` instructions;
    /// 3. update STATUS and increment the tick counter.
    ///
    /// Returns the number of instructions executed (0 once the CPU has
    /// stopped — device registers still update, so a renderer keeps working).
    pub fn tick(&mut self, mem: &mut [u8; MEMORY_SIZE], input: u16, budget: u32) -> u32 {
        write16(mem, mmio::INPUT, input);
        write16(mem, mmio::RNG, self.rng.advance());
        write16(mem, mmio::TICK, self.ticks);

        let steps = self.cpu.run(mem, budget as u64) as u32;

        write16(mem, mmio::STATUS, status_word(self.cpu.state));
        self.ticks = self.ticks.wrapping_add(1);
        steps
    }
}

/// STATUS register encoding: low byte = state, high byte = fault code.
pub fn status_word(state: State) -> u16 {
    match state {
        State::Running => 0,
        State::Halted => 1,
        State::DebugHalt => 2,
        State::Faulted(f) => 3 | ((f.code() as u16) << 8),
    }
}

fn write16(mem: &mut [u8; MEMORY_SIZE], addr: u16, value: u16) {
    let i = addr as usize;
    mem[i..i + 2].copy_from_slice(&value.to_le_bytes());
}
