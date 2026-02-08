use std::env;
use std::process::ExitCode;

use nanopython_selfvm::SelfVm;

fn main() -> ExitCode {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "nanopython-self".to_owned());
    let mut rest: Vec<String> = args.collect();

    if rest.is_empty() {
        print_usage(&program);
        return ExitCode::from(1);
    }

    if rest[0] == "-h" || rest[0] == "--help" {
        print_usage(&program);
        return ExitCode::SUCCESS;
    }

    let mut vm = SelfVm::default();
    if rest[0] == "-c" {
        if rest.len() < 2 {
            eprintln!("nanopython-self: -c requires a command string");
            return ExitCode::from(2);
        }
        vm.run_command(&rest.remove(1))
    } else if rest[0] == "-m" {
        if rest.len() < 2 {
            eprintln!("nanopython-self: -m requires a module name");
            return ExitCode::from(2);
        }
        let module_name = rest[1].clone();
        let module_args = if rest.len() > 2 {
            rest[2..].to_vec()
        } else {
            Vec::new()
        };
        vm.run_module(&module_name, &module_args)
    } else {
        vm.run_script(&rest[0])
    }
}

fn print_usage(program: &str) {
    eprintln!(
        "Usage: {program} [-c CMD | -m MODULE | SCRIPT.py] [args...]\n\
         Self-hosted backend in progress."
    );
}
