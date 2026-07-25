//! CLI for the S16 assembler: `s16-asm <input.s16> [-o <output.bin>]`.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" => output = iter.next(),
            _ if input.is_none() => input = Some(arg),
            _ => {
                eprintln!("s16-asm: unexpected argument `{arg}`");
                return ExitCode::FAILURE;
            }
        }
    }
    let Some(input) = input else {
        eprintln!("usage: s16-asm <input.s16> [-o <output.bin>]");
        return ExitCode::FAILURE;
    };
    let output = output
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&input).with_extension("bin"));

    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s16-asm: cannot read {input}: {e}");
            return ExitCode::FAILURE;
        }
    };

    match s16_asm::assemble(&source) {
        Ok(image) => {
            if let Err(e) = std::fs::write(&output, &image) {
                eprintln!("s16-asm: cannot write {}: {e}", output.display());
                return ExitCode::FAILURE;
            }
            println!("{input} -> {} ({} bytes)", output.display(), image.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{input}:{}: {}", e.line, e.msg);
            ExitCode::FAILURE
        }
    }
}
