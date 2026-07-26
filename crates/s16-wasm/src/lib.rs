//! WASM bindings for s16/s16c: compile s16c source, assemble S16 assembly,
//! and drive a `Machine` tick-by-tick from JS.
//!
//! This crate is a thin wrapper: all determinism and behavior live in
//! `s16-core`/`s16-machine`/`s16-asm`/`s16c`, which are plain library APIs
//! with no OS dependencies. Nothing here duplicates their logic.

use s16_core::MEMORY_SIZE;
use s16_machine::Machine;
use wasm_bindgen::prelude::*;

/// Call once from JS before anything else, to get Rust panics logged to the
/// browser console with a stack trace instead of an opaque abort.
#[wasm_bindgen]
pub fn init_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// Compile s16c source to S16 assembly text (specs/COMPILER.md).
#[wasm_bindgen]
pub fn compile(source: &str) -> Result<String, String> {
    s16c::compile(source).map_err(|e| e.to_string())
}

/// Assemble S16 assembly text into a flat memory image (specs/ASSEMBLY.md).
#[wasm_bindgen]
pub fn assemble(source: &str) -> Result<Vec<u8>, String> {
    s16_asm::assemble(source).map_err(|e| e.to_string())
}

/// Compile s16c source straight through to a runnable memory image.
#[wasm_bindgen]
pub fn compile_and_assemble(source: &str) -> Result<Vec<u8>, String> {
    let asm = compile(source)?;
    assemble(&asm)
}

/// A running S16 machine: CPU plus MMIO devices plus the 64 KB memory image,
/// stepped one tick at a time so a browser host can render state between
/// ticks (specs/MEMORY_MAP.md "Machine Tick").
#[wasm_bindgen]
pub struct WasmMachine {
    machine: Machine,
    mem: Box<[u8; MEMORY_SIZE]>,
}

#[wasm_bindgen]
impl WasmMachine {
    /// Load `image` at address 0 into a fresh 64 KB memory space and reset
    /// the CPU. `rng_seed` seeds the machine's xorshift RNG device.
    #[wasm_bindgen(constructor)]
    pub fn new(image: &[u8], rng_seed: u16) -> WasmMachine {
        let mut mem = Box::new([0u8; MEMORY_SIZE]);
        let n = image.len().min(MEMORY_SIZE);
        mem[..n].copy_from_slice(&image[..n]);
        WasmMachine { machine: Machine::new(rng_seed), mem }
    }

    /// Run one machine tick: service MMIO devices with `input`, execute up
    /// to `budget` instructions, update STATUS. Returns instructions executed
    /// (0 once the CPU has stopped).
    pub fn tick(&mut self, input: u16, budget: u32) -> u32 {
        self.machine.tick(&mut self.mem, input, budget)
    }

    /// The 16 general-purpose registers.
    pub fn registers(&self) -> Vec<u16> {
        self.machine.cpu.regs.to_vec()
    }

    pub fn pc(&self) -> u16 {
        self.machine.cpu.pc
    }

    pub fn sp(&self) -> u16 {
        self.machine.cpu.sp
    }

    pub fn flags(&self) -> u16 {
        self.machine.cpu.flags
    }

    /// STATUS register encoding: low byte = state, high byte = fault code.
    pub fn status(&self) -> u16 {
        s16_machine::status_word(self.machine.cpu.state)
    }

    pub fn ticks(&self) -> u16 {
        self.machine.ticks
    }

    /// Debug-console text written during the most recent tick (D025), empty
    /// if the program wrote nothing this tick.
    pub fn debug_output(&self) -> String {
        String::from_utf8_lossy(self.machine.debug_output()).into_owned()
    }

    /// Read `len` bytes of memory starting at `addr` (e.g. the framebuffer
    /// region), clamped to the end of the 64 KB address space.
    pub fn read_memory(&self, addr: u16, len: u16) -> Vec<u8> {
        let start = addr as usize;
        let end = (start + len as usize).min(MEMORY_SIZE);
        if start >= end {
            return Vec::new();
        }
        self.mem[start..end].to_vec()
    }

    /// Write `bytes` into memory starting at `addr`, clamped to the end of
    /// the 64 KB address space. For host-side pokes outside the normal tick
    /// flow (e.g. loading extra data after construction).
    pub fn write_memory(&mut self, addr: u16, bytes: &[u8]) {
        let start = addr as usize;
        let end = (start + bytes.len()).min(MEMORY_SIZE);
        if start >= end {
            return;
        }
        let n = end - start;
        self.mem[start..start + n].copy_from_slice(&bytes[..n]);
    }
}
