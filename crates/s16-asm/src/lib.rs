//! Two-pass assembler for the S16 assembly language (specs/ASSEMBLY.md).
//!
//! Pass 1 sizes every statement and records label addresses; pass 2 encodes
//! with labels resolved. Opcode values come from `s16_core::opcode` so the
//! assembler and the CPU cannot disagree about the encoding.

use s16_core::opcode as op;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug)]
pub struct AsmError {
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for AsmError {}

/// Assemble S16 source into a flat memory image starting at address 0.
/// The returned vector ends at the highest address written to.
pub fn assemble(source: &str) -> Result<Vec<u8>, AsmError> {
    let lines = parse(source)?;
    let labels = collect_labels(&lines)?;
    emit(&lines, &labels)
}

enum Stmt {
    Instr(String, Vec<String>),
    Org(u16),
    Word(Vec<String>),
    Byte(Vec<String>),
    Ascii(Vec<u8>),
    Space(u16),
}

impl Stmt {
    fn size(&self) -> u32 {
        match self {
            Stmt::Instr(..) => 4,
            Stmt::Org(_) => 0,
            Stmt::Word(v) => 2 * v.len() as u32,
            Stmt::Byte(v) => v.len() as u32,
            Stmt::Ascii(b) => b.len() as u32,
            Stmt::Space(n) => *n as u32,
        }
    }
}

struct Line {
    no: usize,
    labels: Vec<String>,
    stmt: Option<Stmt>,
}

// --- Parsing -----------------------------------------------------------------

fn parse(source: &str) -> Result<Vec<Line>, AsmError> {
    source
        .lines()
        .enumerate()
        .map(|(idx, raw)| parse_line(idx + 1, raw))
        .collect()
}

fn parse_line(no: usize, raw: &str) -> Result<Line, AsmError> {
    let err = |msg: String| AsmError { line: no, msg };
    let mut rest = strip_comment(raw).trim();

    let mut labels = Vec::new();
    while let Some((label, tail)) = split_label(rest) {
        labels.push(label.to_string());
        rest = tail.trim();
    }

    let stmt = if rest.is_empty() {
        None
    } else if let Some(args) = rest.strip_prefix('.') {
        let (name, args) = match args.split_once(char::is_whitespace) {
            Some((n, a)) => (n, a.trim()),
            None => (args, ""),
        };
        Some(parse_directive(&name.to_ascii_lowercase(), args).map_err(err)?)
    } else {
        let (mnemonic, args) = match rest.split_once(char::is_whitespace) {
            Some((m, a)) => (m, a.trim()),
            None => (rest, ""),
        };
        Some(Stmt::Instr(
            mnemonic.to_ascii_uppercase(),
            split_operands(args),
        ))
    };

    Ok(Line { no, labels, stmt })
}

fn parse_directive(name: &str, args: &str) -> Result<Stmt, String> {
    match name {
        "org" => Ok(Stmt::Org(directive_number(args, "org")?)),
        "space" => Ok(Stmt::Space(directive_number(args, "space")?)),
        "word" => Ok(Stmt::Word(nonempty(split_operands(args), ".word")?)),
        "byte" => Ok(Stmt::Byte(nonempty(split_operands(args), ".byte")?)),
        "ascii" => Ok(Stmt::Ascii(parse_string(args)?)),
        "asciiz" => {
            let mut bytes = parse_string(args)?;
            bytes.push(0);
            Ok(Stmt::Ascii(bytes))
        }
        _ => Err(format!("unknown directive `.{name}`")),
    }
}

fn directive_number(s: &str, what: &str) -> Result<u16, String> {
    let v = parse_number(s).ok_or_else(|| format!(".{what} expects a number, got `{s}`"))?;
    if !(0..=0xFFFF).contains(&v) {
        return Err(format!(".{what} value {v} out of range"));
    }
    Ok(v as u16)
}

fn nonempty(v: Vec<String>, what: &str) -> Result<Vec<String>, String> {
    if v.is_empty() || v.iter().any(String::is_empty) {
        Err(format!("{what} expects a comma-separated list of values"))
    } else {
        Ok(v)
    }
}

/// Cut the line at the first `;` that is not inside a string or char literal.
fn strip_comment(s: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quote.is_some() => escaped = true,
            '"' | '\'' => match quote {
                Some(q) if q == c => quote = None,
                None => quote = Some(c),
                _ => {}
            },
            ';' if quote.is_none() => return &s[..i],
            _ => {}
        }
    }
    s
}

/// If `s` starts with `ident:`, return (ident, rest-after-colon).
fn split_label(s: &str) -> Option<(&str, &str)> {
    let end = s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
    let head = &s[..end];
    if end == 0
        || s.as_bytes()[end] != b':'
        || !head.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
    {
        return None;
    }
    Some((head, &s[end + 1..]))
}

/// Split on commas that are not inside string/char literals.
fn split_operands(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            cur.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if quote.is_some() => {
                cur.push(c);
                escaped = true;
            }
            '"' | '\'' => {
                match quote {
                    Some(q) if q == c => quote = None,
                    None => quote = Some(c),
                    _ => {}
                }
                cur.push(c);
            }
            ',' if quote.is_none() => {
                parts.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    parts.push(cur.trim().to_string());
    parts
}

// --- Values ------------------------------------------------------------------

/// Decimal, hex (`0x2A`), or char (`'A'`) literal, with optional leading `-`.
fn parse_number(s: &str) -> Option<i64> {
    let s = s.trim();
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s),
    };
    let v = if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()?
    } else if body.starts_with('\'') {
        parse_char(body)? as i64
    } else {
        body.parse::<i64>().ok()?
    };
    Some(if neg { -v } else { v })
}

fn unescape(c: char) -> Option<u8> {
    Some(match c {
        'n' => b'\n',
        't' => b'\t',
        'r' => b'\r',
        '0' => 0,
        '\\' => b'\\',
        '\'' => b'\'',
        '"' => b'"',
        _ => return None,
    })
}

fn parse_char(s: &str) -> Option<u8> {
    let inner = s.strip_prefix('\'')?.strip_suffix('\'')?;
    let mut chars = inner.chars();
    let b = match chars.next()? {
        '\\' => unescape(chars.next()?)?,
        c if c.is_ascii() => c as u8,
        _ => return None,
    };
    match chars.next() {
        None => Some(b),
        Some(_) => None,
    }
}

fn parse_string(s: &str) -> Result<Vec<u8>, String> {
    let bad = || format!("expected a double-quoted ASCII string, got `{s}`");
    let inner = s
        .trim()
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .ok_or_else(bad)?;
    let mut out = Vec::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        let b = match c {
            '\\' => chars.next().and_then(unescape).ok_or_else(bad)?,
            c if c.is_ascii() => c as u8,
            _ => return Err(bad()),
        };
        out.push(b);
    }
    Ok(out)
}

/// A 16-bit value: a number literal or a label address.
fn value(s: &str, labels: &HashMap<String, u16>) -> Result<u16, String> {
    let s = s.trim();
    if let Some(v) = parse_number(s) {
        if !(-32768..=65535).contains(&v) {
            return Err(format!("value {v} does not fit in 16 bits"));
        }
        Ok(v as u16)
    } else if let Some(&addr) = labels.get(s) {
        Ok(addr)
    } else {
        Err(format!("undefined label or bad value `{s}`"))
    }
}

fn register(s: &str) -> Option<u16> {
    let digits = s.trim().strip_prefix(['R', 'r'])?;
    let n: u16 = digits.parse().ok()?;
    if n < 16 {
        Some(n)
    } else {
        None
    }
}

/// `[Rn]`, `[Rn+off]`, or `[Rn-off]` -> (base register, offset as u16).
fn mem_operand(s: &str) -> Option<(u16, u16)> {
    let inner = s.trim().strip_prefix('[')?.strip_suffix(']')?.trim();
    let (reg_part, off) = match inner.find(['+', '-']) {
        Some(pos) => {
            let (r, rest) = inner.split_at(pos);
            let off = parse_number(rest.strip_prefix('+').unwrap_or(rest))?;
            if !(-32768..=65535).contains(&off) {
                return None;
            }
            (r, off as u16)
        }
        None => (inner, 0),
    };
    Some((register(reg_part)?, off))
}

// --- Passes ------------------------------------------------------------------

fn collect_labels(lines: &[Line]) -> Result<HashMap<String, u16>, AsmError> {
    let mut labels = HashMap::new();
    let mut addr: u32 = 0;
    for line in lines {
        let err = |msg: String| AsmError { line: line.no, msg };
        for label in &line.labels {
            if addr > 0xFFFF {
                return Err(err(format!("label `{label}` is past the end of memory")));
            }
            if labels.insert(label.clone(), addr as u16).is_some() {
                return Err(err(format!("duplicate label `{label}`")));
            }
        }
        if let Some(stmt) = &line.stmt {
            match stmt {
                Stmt::Org(a) => addr = *a as u32,
                _ => addr += stmt.size(),
            }
            if addr > 0x1_0000 {
                return Err(err("output runs past the end of memory".into()));
            }
        }
    }
    Ok(labels)
}

fn emit(lines: &[Line], labels: &HashMap<String, u16>) -> Result<Vec<u8>, AsmError> {
    let mut mem = vec![0u8; 0x1_0000];
    let mut addr: u32 = 0;
    let mut end: usize = 0;

    for line in lines {
        let err = |msg: String| AsmError { line: line.no, msg };
        let Some(stmt) = &line.stmt else { continue };

        let mut push = |bytes: &[u8], addr: &mut u32| {
            mem[*addr as usize..*addr as usize + bytes.len()].copy_from_slice(bytes);
            *addr += bytes.len() as u32;
        };

        // collect_labels already bounds-checked every address.
        match stmt {
            Stmt::Org(a) => addr = *a as u32,
            Stmt::Instr(m, ops) => {
                if !addr.is_multiple_of(4) {
                    return Err(err(format!(
                        "instruction at {addr:#06X} is not 4-byte aligned (use .org/.space to realign)"
                    )));
                }
                let word = encode(m, ops, addr as u16, labels).map_err(err)?;
                push(&word.to_le_bytes(), &mut addr);
            }
            Stmt::Word(vals) => {
                for v in vals {
                    let x = value(v, labels).map_err(err)?;
                    push(&x.to_le_bytes(), &mut addr);
                }
            }
            Stmt::Byte(vals) => {
                for v in vals {
                    let x = parse_number(v)
                        .filter(|x| (-128..=255).contains(x))
                        .ok_or_else(|| err(format!("`{v}` is not a valid byte value")))?;
                    push(&[x as u8], &mut addr);
                }
            }
            Stmt::Ascii(bytes) => push(bytes, &mut addr),
            Stmt::Space(n) => addr += *n as u32,
        }
        end = end.max(addr as usize);
    }

    mem.truncate(end);
    Ok(mem)
}

// --- Instruction encoding ----------------------------------------------------

fn r_type(opc: u8, rd: u16, rs1: u16, rs2: u16) -> u32 {
    (opc as u32) << 24 | (rd as u32) << 20 | (rs1 as u32) << 16 | (rs2 as u32) << 12
}

fn i_type(opc: u8, rd: u16, rs1: u16, imm: u16) -> u32 {
    (opc as u32) << 24 | (rd as u32) << 20 | (rs1 as u32) << 16 | imm as u32
}

fn j_type(opc: u8, offset: u16) -> u32 {
    (opc as u32) << 24 | offset as u32
}

fn encode(
    m: &str,
    ops: &[String],
    addr: u16,
    labels: &HashMap<String, u16>,
) -> Result<u32, String> {
    let need = |n: usize| {
        if ops.len() == n {
            Ok(())
        } else {
            Err(format!("{m} expects {n} operand(s), got {}", ops.len()))
        }
    };
    let reg = |i: usize| {
        register(&ops[i])
            .ok_or_else(|| format!("operand {} of {m}: expected a register, got `{}`", i + 1, ops[i]))
    };
    let imm = |i: usize| -> Result<u16, String> {
        let s = ops[i]
            .strip_prefix('#')
            .ok_or_else(|| format!("operand {} of {m}: immediates start with '#'", i + 1))?;
        value(s, labels)
    };
    let mem = |i: usize| {
        mem_operand(&ops[i]).ok_or_else(|| {
            format!("operand {} of {m}: expected `[Rn]` or `[Rn+off]`, got `{}`", i + 1, ops[i])
        })
    };
    // A label becomes a PC-relative instruction offset; `#n` is a raw offset.
    let target = |i: usize| -> Result<u16, String> {
        let s = ops[i].trim();
        if let Some(raw) = s.strip_prefix('#') {
            let v = parse_number(raw)
                .filter(|v| (-32768..=32767).contains(v))
                .ok_or_else(|| format!("bad branch offset `{s}`"))?;
            return Ok(v as u16);
        }
        let t = *labels.get(s).ok_or_else(|| format!("undefined label `{s}`"))?;
        let diff = t as i32 - (addr as i32 + 4);
        if diff % 4 != 0 {
            return Err(format!("branch target `{s}` ({t:#06X}) is not 4-byte aligned"));
        }
        Ok(((diff / 4) as i16) as u16)
    };

    let word = match m {
        "NOP" | "HALT" | "BRK" | "WAIT" | "RET" => {
            need(0)?;
            let opc = match m {
                "NOP" => op::NOP,
                "HALT" => op::HALT,
                "BRK" => op::BRK,
                "WAIT" => op::WAIT,
                _ => op::RET,
            };
            r_type(opc, 0, 0, 0)
        }
        "MOV" => {
            need(2)?;
            r_type(op::MOV, reg(0)?, reg(1)?, 0)
        }
        "LDI" => {
            need(2)?;
            i_type(op::LDI, reg(0)?, 0, imm(1)?)
        }
        "LDW" | "LDB" => {
            need(2)?;
            let (base, off) = mem(1)?;
            i_type(if m == "LDW" { op::LDW } else { op::LDB }, reg(0)?, base, off)
        }
        "STW" | "STB" => {
            need(2)?;
            let (base, off) = mem(0)?;
            i_type(if m == "STW" { op::STW } else { op::STB }, reg(1)?, base, off)
        }
        "ADD" | "SUB" | "MUL" | "DIV" | "MOD" | "AND" | "OR" | "XOR" | "SHL" | "SHR" | "SAR" => {
            need(3)?;
            let opc = match m {
                "ADD" => op::ADD,
                "SUB" => op::SUB,
                "MUL" => op::MUL,
                "DIV" => op::DIV,
                "MOD" => op::MOD,
                "AND" => op::AND,
                "OR" => op::OR,
                "XOR" => op::XOR,
                "SHL" => op::SHL,
                "SHR" => op::SHR,
                _ => op::SAR,
            };
            r_type(opc, reg(0)?, reg(1)?, reg(2)?)
        }
        "NOT" => {
            need(2)?;
            r_type(op::NOT, reg(0)?, reg(1)?, 0)
        }
        "ADDI" | "SUBI" | "ANDI" | "ORI" | "XORI" | "SHLI" | "SHRI" | "SARI" => {
            need(3)?;
            let opc = match m {
                "ADDI" => op::ADDI,
                "SUBI" => op::SUBI,
                "ANDI" => op::ANDI,
                "ORI" => op::ORI,
                "XORI" => op::XORI,
                "SHLI" => op::SHLI,
                "SHRI" => op::SHRI,
                _ => op::SARI,
            };
            i_type(opc, reg(0)?, reg(1)?, imm(2)?)
        }
        "CMP" => {
            need(2)?;
            r_type(op::CMP, 0, reg(0)?, reg(1)?)
        }
        "CMPI" => {
            need(2)?;
            i_type(op::CMPI, 0, reg(0)?, imm(1)?)
        }
        "JMP" | "BEQ" | "BNE" | "BLT" | "BGE" | "BLTU" | "BGEU" | "CALL" => {
            need(1)?;
            let opc = match m {
                "JMP" => op::JMP,
                "BEQ" => op::BEQ,
                "BNE" => op::BNE,
                "BLT" => op::BLT,
                "BGE" => op::BGE,
                "BLTU" => op::BLTU,
                "BGEU" => op::BGEU,
                _ => op::CALL,
            };
            j_type(opc, target(0)?)
        }
        "JR" => {
            need(1)?;
            r_type(op::JR, 0, reg(0)?, 0)
        }
        "PUSH" => {
            need(1)?;
            r_type(op::PUSH, 0, reg(0)?, 0)
        }
        "POP" => {
            need(1)?;
            r_type(op::POP, reg(0)?, 0, 0)
        }
        "RSP" => {
            need(1)?;
            r_type(op::RSP, reg(0)?, 0, 0)
        }
        "WSP" => {
            need(1)?;
            r_type(op::WSP, 0, reg(0)?, 0)
        }
        _ => return Err(format!("unknown mnemonic `{m}`")),
    };
    Ok(word)
}
