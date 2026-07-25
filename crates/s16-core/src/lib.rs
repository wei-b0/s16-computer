//! The S16 CPU core.
//!
//! Implements the architecture specified in `specs/` (ARCHITECTURE.md,
//! INSTRUCTION_SET.md, MEMORY_MAP.md). This crate must stay deterministic and
//! dependency-free: no clocks, no OS randomness, no floating point, no I/O —
//! devices are plain memory (MMIO). The same code runs in the local emulator
//! and inside the Solana program.
//!
//! The CPU does not own memory; `step` borrows the 64 KB image so hosts can
//! keep it wherever they need (heap locally, account data on Solana). Device
//! semantics for the MMIO registers (INPUT, RNG, TICK) are a host concern:
//! hosts update those addresses between steps.

#![no_std]

/// Memory size: 64 KB, byte-addressable.
pub const MEMORY_SIZE: usize = 0x1_0000;

/// Reset value of the stack pointer; the stack grows downward from here.
pub const SP_RESET: u16 = 0xF000;

/// FLAGS bits, written only by CMP/CMPI.
pub const FLAG_Z: u16 = 1 << 0;
pub const FLAG_N: u16 = 1 << 1;
pub const FLAG_C: u16 = 1 << 2;
pub const FLAG_V: u16 = 1 << 3;

/// Opcode values from INSTRUCTION_SET.md. Shared with the assembler.
pub mod opcode {
    pub const NOP: u8 = 0x00;
    pub const HALT: u8 = 0x01;
    pub const BRK: u8 = 0x02;

    pub const MOV: u8 = 0x10;
    pub const LDI: u8 = 0x11;
    pub const LDW: u8 = 0x12;
    pub const LDB: u8 = 0x13;
    pub const STW: u8 = 0x14;
    pub const STB: u8 = 0x15;

    pub const ADD: u8 = 0x20;
    pub const SUB: u8 = 0x21;
    pub const MUL: u8 = 0x22;
    pub const DIV: u8 = 0x23;
    pub const MOD: u8 = 0x24;
    pub const AND: u8 = 0x25;
    pub const OR: u8 = 0x26;
    pub const XOR: u8 = 0x27;
    pub const NOT: u8 = 0x28;
    pub const SHL: u8 = 0x29;
    pub const SHR: u8 = 0x2A;
    pub const SAR: u8 = 0x2B;

    pub const ADDI: u8 = 0x30;
    pub const SUBI: u8 = 0x31;
    pub const ANDI: u8 = 0x32;
    pub const ORI: u8 = 0x33;
    pub const XORI: u8 = 0x34;
    pub const SHLI: u8 = 0x35;
    pub const SHRI: u8 = 0x36;
    pub const SARI: u8 = 0x37;

    pub const CMP: u8 = 0x40;
    pub const CMPI: u8 = 0x41;

    pub const JMP: u8 = 0x50;
    pub const JR: u8 = 0x51;
    pub const BEQ: u8 = 0x52;
    pub const BNE: u8 = 0x53;
    pub const BLT: u8 = 0x54;
    pub const BGE: u8 = 0x55;
    pub const BLTU: u8 = 0x56;
    pub const BGEU: u8 = 0x57;
    pub const CALL: u8 = 0x58;
    pub const RET: u8 = 0x59;

    pub const PUSH: u8 = 0x60;
    pub const POP: u8 = 0x61;
}

/// Fault codes (see INSTRUCTION_SET.md "Faults").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    IllegalOpcode = 1,
    UnalignedAccess = 2,
    DivByZero = 3,
}

impl Fault {
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// CPU execution state (see INSTRUCTION_SET.md "Execution States").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Halted,
    DebugHalt,
    Faulted(Fault),
}

/// The S16 CPU: registers and execution state. Memory is borrowed per step.
#[derive(Debug, Clone)]
pub struct Cpu {
    pub regs: [u16; 16],
    pub pc: u16,
    pub sp: u16,
    pub flags: u16,
    pub state: State,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    /// A CPU in the reset state defined in MEMORY_MAP.md.
    pub fn new() -> Self {
        Cpu {
            regs: [0; 16],
            pc: 0x0000,
            sp: SP_RESET,
            flags: 0,
            state: State::Running,
        }
    }

    /// Execute one instruction. Does nothing unless the state is Running.
    ///
    /// After HALT/BRK, PC points past the stopping instruction (so a debugger
    /// resuming from BRK continues naturally). After a fault, PC still points
    /// at the faulting instruction and no partial register/memory/SP updates
    /// from it are visible.
    pub fn step(&mut self, mem: &mut [u8; MEMORY_SIZE]) {
        if self.state != State::Running {
            return;
        }
        if let Err(fault) = self.exec(mem) {
            self.state = State::Faulted(fault);
        }
    }

    /// Step until the CPU stops or `max_steps` is reached. Returns the number
    /// of instructions executed.
    pub fn run(&mut self, mem: &mut [u8; MEMORY_SIZE], max_steps: u64) -> u64 {
        let mut steps = 0;
        while steps < max_steps && self.state == State::Running {
            self.step(mem);
            steps += 1;
        }
        steps
    }

    fn exec(&mut self, mem: &mut [u8; MEMORY_SIZE]) -> Result<(), Fault> {
        // PC is always 4-byte aligned: it starts at 0 and every update adds a
        // multiple of 4 (mod 0x10000) or is alignment-checked (JR/RET).
        let w = fetch32(mem, self.pc);
        let op = (w >> 24) as u8;
        let rd = ((w >> 20) & 0xF) as usize;
        let rs1 = ((w >> 16) & 0xF) as usize;
        let rs2 = ((w >> 12) & 0xF) as usize;
        let imm = w as u16;

        // `next` is the PC after this instruction; branch arms overwrite it.
        let mut next = self.pc.wrapping_add(4);

        use opcode::*;
        match op {
            NOP => {}
            HALT => self.state = State::Halted,
            BRK => self.state = State::DebugHalt,

            MOV => self.regs[rd] = self.regs[rs1],
            LDI => self.regs[rd] = imm,
            // Adding the raw imm16 with wraparound equals adding its
            // sign-extended value mod 2^16, so no explicit sign extension.
            LDW => self.regs[rd] = load16(mem, self.regs[rs1].wrapping_add(imm))?,
            LDB => self.regs[rd] = mem[self.regs[rs1].wrapping_add(imm) as usize] as u16,
            STW => store16(mem, self.regs[rs1].wrapping_add(imm), self.regs[rd])?,
            STB => mem[self.regs[rs1].wrapping_add(imm) as usize] = self.regs[rd] as u8,

            ADD => self.regs[rd] = self.regs[rs1].wrapping_add(self.regs[rs2]),
            SUB => self.regs[rd] = self.regs[rs1].wrapping_sub(self.regs[rs2]),
            MUL => self.regs[rd] = self.regs[rs1].wrapping_mul(self.regs[rs2]),
            DIV => self.regs[rd] = self.regs[rs1] / nonzero(self.regs[rs2])?,
            MOD => self.regs[rd] = self.regs[rs1] % nonzero(self.regs[rs2])?,
            AND => self.regs[rd] = self.regs[rs1] & self.regs[rs2],
            OR => self.regs[rd] = self.regs[rs1] | self.regs[rs2],
            XOR => self.regs[rd] = self.regs[rs1] ^ self.regs[rs2],
            NOT => self.regs[rd] = !self.regs[rs1],
            SHL => self.regs[rd] = shl(self.regs[rs1], self.regs[rs2]),
            SHR => self.regs[rd] = shr(self.regs[rs1], self.regs[rs2]),
            SAR => self.regs[rd] = sar(self.regs[rs1], self.regs[rs2]),

            ADDI => self.regs[rd] = self.regs[rs1].wrapping_add(imm),
            SUBI => self.regs[rd] = self.regs[rs1].wrapping_sub(imm),
            ANDI => self.regs[rd] = self.regs[rs1] & imm,
            ORI => self.regs[rd] = self.regs[rs1] | imm,
            XORI => self.regs[rd] = self.regs[rs1] ^ imm,
            SHLI => self.regs[rd] = shl(self.regs[rs1], imm),
            SHRI => self.regs[rd] = shr(self.regs[rs1], imm),
            SARI => self.regs[rd] = sar(self.regs[rs1], imm),

            CMP => self.flags = cmp_flags(self.regs[rs1], self.regs[rs2]),
            CMPI => self.flags = cmp_flags(self.regs[rs1], imm),

            JMP => next = branch_target(self.pc, imm),
            JR => {
                let target = self.regs[rs1];
                if target & 3 != 0 {
                    return Err(Fault::UnalignedAccess);
                }
                next = target;
            }
            BEQ => branch_if(self.flags & FLAG_Z != 0, &mut next, self.pc, imm),
            BNE => branch_if(self.flags & FLAG_Z == 0, &mut next, self.pc, imm),
            BLT => branch_if(
                (self.flags & FLAG_N != 0) != (self.flags & FLAG_V != 0),
                &mut next,
                self.pc,
                imm,
            ),
            BGE => branch_if(
                (self.flags & FLAG_N != 0) == (self.flags & FLAG_V != 0),
                &mut next,
                self.pc,
                imm,
            ),
            BLTU => branch_if(self.flags & FLAG_C != 0, &mut next, self.pc, imm),
            BGEU => branch_if(self.flags & FLAG_C == 0, &mut next, self.pc, imm),
            CALL => {
                let new_sp = self.sp.wrapping_sub(2);
                store16(mem, new_sp, next)?;
                self.sp = new_sp;
                next = branch_target(self.pc, imm);
            }
            RET => {
                let target = load16(mem, self.sp)?;
                if target & 3 != 0 {
                    return Err(Fault::UnalignedAccess);
                }
                self.sp = self.sp.wrapping_add(2);
                next = target;
            }

            PUSH => {
                let new_sp = self.sp.wrapping_sub(2);
                store16(mem, new_sp, self.regs[rs1])?;
                self.sp = new_sp;
            }
            POP => {
                let value = load16(mem, self.sp)?;
                self.sp = self.sp.wrapping_add(2);
                self.regs[rd] = value;
            }

            _ => return Err(Fault::IllegalOpcode),
        }

        self.pc = next;
        Ok(())
    }
}

fn fetch32(mem: &[u8; MEMORY_SIZE], pc: u16) -> u32 {
    let i = pc as usize; // 4-byte aligned, so i+3 <= 0xFFFF
    u32::from_le_bytes([mem[i], mem[i + 1], mem[i + 2], mem[i + 3]])
}

fn load16(mem: &[u8; MEMORY_SIZE], addr: u16) -> Result<u16, Fault> {
    if addr & 1 != 0 {
        return Err(Fault::UnalignedAccess);
    }
    let i = addr as usize;
    Ok(u16::from_le_bytes([mem[i], mem[i + 1]]))
}

fn store16(mem: &mut [u8; MEMORY_SIZE], addr: u16, value: u16) -> Result<(), Fault> {
    if addr & 1 != 0 {
        return Err(Fault::UnalignedAccess);
    }
    let i = addr as usize;
    mem[i..i + 2].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn nonzero(divisor: u16) -> Result<u16, Fault> {
    if divisor == 0 {
        Err(Fault::DivByZero)
    } else {
        Ok(divisor)
    }
}

/// FLAGS from `a - b` (see INSTRUCTION_SET.md "FLAGS Register").
fn cmp_flags(a: u16, b: u16) -> u16 {
    let r = a.wrapping_sub(b);
    let mut f = 0;
    if r == 0 {
        f |= FLAG_Z;
    }
    if r & 0x8000 != 0 {
        f |= FLAG_N;
    }
    if a < b {
        f |= FLAG_C;
    }
    if (a ^ b) & (a ^ r) & 0x8000 != 0 {
        f |= FLAG_V;
    }
    f
}

/// `target = PC + 4 + offset*4`, offset in instructions. Wrapping the raw
/// imm16 arithmetic mod 2^16 handles the signedness for free.
fn branch_target(pc: u16, imm: u16) -> u16 {
    pc.wrapping_add(4).wrapping_add(imm.wrapping_mul(4))
}

fn branch_if(cond: bool, next: &mut u16, pc: u16, imm: u16) {
    if cond {
        *next = branch_target(pc, imm);
    }
}

fn shl(v: u16, amt: u16) -> u16 {
    if amt >= 16 {
        0
    } else {
        v << amt
    }
}

fn shr(v: u16, amt: u16) -> u16 {
    if amt >= 16 {
        0
    } else {
        v >> amt
    }
}

fn sar(v: u16, amt: u16) -> u16 {
    if amt >= 16 {
        if v & 0x8000 != 0 {
            0xFFFF
        } else {
            0
        }
    } else {
        ((v as i16) >> amt) as u16
    }
}
