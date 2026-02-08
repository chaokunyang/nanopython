//! nanopython-compiler currently delegates bytecode compilation to the VM backend.
//! This crate defines a stable boundary for future pluggable compiler crates.

use nanopython_core::Result;
use nanopython_parser::ParsedModule;

#[derive(Debug, Clone)]
pub struct CompiledModule {
    pub stmt_count: usize,
}

pub fn compile_module(parsed: ParsedModule) -> Result<CompiledModule> {
    Ok(CompiledModule {
        stmt_count: parsed.body.len(),
    })
}
