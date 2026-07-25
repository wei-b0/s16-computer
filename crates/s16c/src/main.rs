//! CLI for the s16c compiler: `s16c <input.s16c> [-o <output>] [--bin]`.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut bin = false;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" => output = iter.next(),
            "--bin" => bin = true,
            _ if input.is_none() && !arg.starts_with('-') => input = Some(arg),
            _ => {
                eprintln!("s16c: unexpected argument `{arg}`");
                return ExitCode::FAILURE;
            }
        }
    }
    let Some(input) = input else {
        eprintln!("usage: s16c <input.s16c> [-o <output>] [--bin]");
        return ExitCode::FAILURE;
    };
    let output = output.map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(&input).with_extension(if bin { "bin" } else { "s16" })
    });

    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s16c: cannot read {input}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let asm = match s16c::compile(&source) {
        Ok(asm) => asm,
        Err(e) => {
            eprintln!("{input}:{e}");
            return ExitCode::FAILURE;
        }
    };

    let bytes: Vec<u8> = if bin {
        match s16_asm::assemble(&asm) {
            Ok(image) => image,
            Err(e) => {
                // A compiler bug, not a user error: surface it clearly.
                eprintln!("s16c: internal error: emitted assembly does not assemble: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        asm.into_bytes()
    };

    if let Err(e) = std::fs::write(&output, &bytes) {
        eprintln!("s16c: cannot write {}: {e}", output.display());
        return ExitCode::FAILURE;
    }
    println!("{input} -> {} ({} bytes)", output.display(), bytes.len());
    ExitCode::SUCCESS
}
