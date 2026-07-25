//! Interactive machine mode: render the framebuffer to the terminal each
//! tick and feed the keyboard into the INPUT register.
//!
//! One machine tick per rendered frame (~30 fps). All machine semantics live
//! in s16-machine; this module only draws cells and collects key state.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{Attribute, Color, Colors, Print, SetAttribute, SetColors},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use s16_core::{State, MEMORY_SIZE};
use s16_machine::{mmio, Machine};

const FRAME: Duration = Duration::from_millis(33); // ~30 ticks/second
/// Terminals rarely report key releases; a key counts as held for this long
/// after its last press/repeat event.
const KEY_HOLD: Duration = Duration::from_millis(200);

pub fn run(image: &[u8], seed: u16, budget: u32) -> io::Result<(Machine, u64)> {
    let mut mem = vec![0u8; MEMORY_SIZE];
    mem[..image.len()].copy_from_slice(image);
    let mut mem: Box<[u8; MEMORY_SIZE]> = mem.into_boxed_slice().try_into().unwrap();

    let mut out = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(out, EnterAlternateScreen, Hide, Clear(ClearType::All))?;
    let result = machine_loop(&mut out, &mut mem, seed, budget);
    execute!(out, LeaveAlternateScreen, Show)?;
    terminal::disable_raw_mode()?;
    result
}

fn machine_loop(
    out: &mut io::Stdout,
    mem: &mut [u8; MEMORY_SIZE],
    seed: u16,
    budget: u32,
) -> io::Result<(Machine, u64)> {
    let mut machine = Machine::new(seed);
    let mut keys = Keys::default();
    let mut executed: u64 = 0;

    loop {
        let deadline = Instant::now() + FRAME;
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            if !event::poll(timeout)? {
                break;
            }
            let Event::Key(key) = event::read()? else { continue };
            match key.code {
                KeyCode::Esc => return Ok((machine, executed)),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok((machine, executed));
                }
                KeyCode::Char('r') if machine.cpu.state == State::DebugHalt => {
                    machine.cpu.state = State::Running;
                }
                code => {
                    if let Some(bit) = key_bit(code) {
                        match key.kind {
                            KeyEventKind::Release => keys.release(bit),
                            _ => keys.press(bit),
                        }
                    }
                }
            }
        }

        executed += machine.tick(mem, keys.mask(Instant::now()), budget) as u64;
        draw(out, mem, &machine)?;
    }
}

/// Key state with hold decay, since most terminals never send key-up.
#[derive(Default)]
struct Keys {
    last_press: [Option<Instant>; 8],
}

impl Keys {
    fn press(&mut self, bit: usize) {
        self.last_press[bit] = Some(Instant::now());
    }
    fn release(&mut self, bit: usize) {
        self.last_press[bit] = None;
    }
    fn mask(&self, now: Instant) -> u16 {
        self.last_press
            .iter()
            .enumerate()
            .fold(0, |mask, (bit, pressed)| match pressed {
                Some(at) if now.duration_since(*at) < KEY_HOLD => mask | 1 << bit,
                _ => mask,
            })
    }
}

/// Arrows/WASD -> d-pad, Z/Space -> A, X -> B, Enter -> Start, Tab -> Select.
fn key_bit(code: KeyCode) -> Option<usize> {
    Some(match code {
        KeyCode::Up | KeyCode::Char('w') => 0,
        KeyCode::Down | KeyCode::Char('s') => 1,
        KeyCode::Left | KeyCode::Char('a') => 2,
        KeyCode::Right | KeyCode::Char('d') => 3,
        KeyCode::Char('z') | KeyCode::Char(' ') => 4,
        KeyCode::Char('x') => 5,
        KeyCode::Enter => 6,
        KeyCode::Tab => 7,
        _ => return None,
    })
}

fn cell(mem: &[u8; MEMORY_SIZE], row: u16, col: u16) -> (u8, u8) {
    let i = mmio::cell_addr(row, col) as usize;
    (mem[i], mem[i + 1])
}

fn printable(ch: u8) -> char {
    match ch {
        0 => ' ',
        0x20..=0x7E => ch as char,
        _ => '?',
    }
}

fn draw(out: &mut io::Stdout, mem: &[u8; MEMORY_SIZE], machine: &Machine) -> io::Result<()> {
    let mut current_attr: Option<u8> = None;
    for row in 0..mmio::FB_ROWS {
        queue!(out, MoveTo(0, row))?;
        for col in 0..mmio::FB_COLS {
            let (ch, attr) = cell(mem, row, col);
            if current_attr != Some(attr) {
                // attribute byte: bits 0-3 fg, 4-6 bg, 7 blink
                queue!(
                    out,
                    SetAttribute(Attribute::Reset),
                    SetColors(Colors::new(
                        Color::AnsiValue(attr & 0x0F),
                        Color::AnsiValue(attr >> 4 & 0x07),
                    ))
                )?;
                if attr & 0x80 != 0 {
                    queue!(out, SetAttribute(Attribute::SlowBlink))?;
                }
                current_attr = Some(attr);
            }
            queue!(out, Print(printable(ch)))?;
        }
    }

    let state = match machine.cpu.state {
        State::Running => "running".to_string(),
        State::Halted => "halted".to_string(),
        State::DebugHalt => "BRK - press r to resume".to_string(),
        State::Faulted(f) => format!("FAULT {} ({f:?}) at PC={:#06X}", f.code(), machine.cpu.pc),
    };
    let status = format!(" S16 | tick {:5} | {state} | esc quits ", machine.ticks);
    queue!(
        out,
        SetAttribute(Attribute::Reset),
        MoveTo(0, mmio::FB_ROWS),
        Clear(ClearType::CurrentLine),
        SetAttribute(Attribute::Reverse),
        Print(status),
        SetAttribute(Attribute::Reset),
    )?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain-text view of the framebuffer, for tests.
    fn frame_text(mem: &[u8; MEMORY_SIZE]) -> String {
        let mut text = String::new();
        for row in 0..mmio::FB_ROWS {
            for col in 0..mmio::FB_COLS {
                text.push(printable(cell(mem, row, col).0));
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn frame_text_reads_cells() {
        let mut mem = vec![0u8; MEMORY_SIZE];
        for (i, ch) in b"HI".iter().enumerate() {
            mem[mmio::cell_addr(0, i as u16) as usize] = *ch;
        }
        mem[mmio::cell_addr(24, 39) as usize] = b'!';
        let mem: Box<[u8; MEMORY_SIZE]> = mem.into_boxed_slice().try_into().unwrap();

        let text = frame_text(&mem);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 25);
        assert!(lines[0].starts_with("HI  "));
        assert!(lines[24].ends_with('!'));
    }

    #[test]
    fn nonprintable_bytes_render_safely() {
        assert_eq!(printable(0), ' ');
        assert_eq!(printable(b'A'), 'A');
        assert_eq!(printable(0x1B), '?', "escape byte must not reach the terminal");
        assert_eq!(printable(0xFF), '?');
    }
}
