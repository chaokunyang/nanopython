mod ast;
mod lexer;
mod parser;
mod runtime;
mod token;

use runtime::Interpreter;
use std::env;
use std::path::{Path, PathBuf};
use std::process;

fn usage() {
    eprintln!(
        "usage: nanopy [-m module | -c code | script.py | package_dir] [args...]\n\
         phase1: custom interpreter scaffold (no rustpython, no host cpython)"
    );
}

fn env_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        paths.push(cwd);
    }
    if let Some(raw) = env::var_os("PYTHONPATH") {
        for p in env::split_paths(&raw) {
            paths.push(p);
        }
    }
    paths
}

fn resolve_script_target(path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_dir() {
        let candidate = p.join("__main__.py");
        if candidate.is_file() {
            return candidate;
        }
    }
    p.to_path_buf()
}

fn run() -> Result<i32, String> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() == 1 || args[1] == "-h" || args[1] == "--help" {
        usage();
        return Ok(0);
    }

    let mut interp = Interpreter::new();
    interp.set_search_paths(env_search_paths());

    match args[1].as_str() {
        "-c" => {
            if args.len() < 3 {
                return Err("nanopy: -c requires code string".to_owned());
            }
            interp.set_argv(vec!["-c".to_owned()]);
            let _ = interp.run_source(&args[2], "__main__")?;
            Ok(0)
        }
        "-m" => {
            if args.len() < 3 {
                return Err("nanopy: -m requires module name".to_owned());
            }
            let mut argv = vec![args[2].clone()];
            argv.extend(args[3..].iter().cloned());
            interp.set_argv(argv);
            interp.run_module(&args[2])?;
            Ok(0)
        }
        _ => {
            let script_path = resolve_script_target(&args[1]);
            if !script_path.is_file() {
                return Err(format!(
                    "nanopy: script not found '{}'",
                    script_path.to_string_lossy()
                ));
            }
            let argv = args[1..].to_vec();
            interp.call_main(&script_path, &argv)
        }
    }
}

fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    }
}
