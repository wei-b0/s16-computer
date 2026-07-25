//! The s16c compiler: specs/LANGUAGE.md in, S16 assembly text out
//! (specs/COMPILER.md is the contract, specs/CALLING_CONVENTION.md the ABI).

mod ast;
mod check;
mod codegen;
mod lexer;
mod parser;

use std::fmt;

#[derive(Debug)]
pub struct CompileError {
    pub line: usize,
    pub col: usize,
    pub msg: String,
}

impl CompileError {
    fn at(pos: Pos, msg: String) -> Self {
        CompileError { line: pos.line, col: pos.col, msg }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.msg)
    }
}

impl std::error::Error for CompileError {}

/// A 1-indexed source position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

/// Compile s16c source to S16 assembly text.
pub fn compile(source: &str) -> Result<String, CompileError> {
    let tokens = lexer::lex(source)?;
    let items = parser::parse(&tokens)?;
    let program = check::check(&items)?;
    Ok(codegen::generate(&program))
}
