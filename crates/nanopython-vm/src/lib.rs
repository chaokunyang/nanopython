use std::{env, path::PathBuf, process::ExitCode};

use nanopython_plugin_api::PluginRegistry;
use rustpython_vm::{Interpreter, PyResult, Settings, VirtualMachine, scope::Scope};

pub struct NanoPythonVm {
    plugins: PluginRegistry,
}

enum RunMode {
    Command(String),
    Module(String),
    Script(String),
}

impl NanoPythonVm {
    pub fn new(plugins: PluginRegistry) -> Self {
        Self { plugins }
    }

    pub fn run(self) -> ExitCode {
        self.run_with_args(env::args().collect())
    }

    pub fn run_with_args(self, args: Vec<String>) -> ExitCode {
        if let Err(err) = self.plugins.validate() {
            eprintln!("nanopython plugin validation error: {err}");
            return ExitCode::from(2);
        }

        let (settings, mode) = match parse_cli(args) {
            Ok(v) => v,
            Err(exit) => return exit,
        };

        let plugins = self.plugins;
        let interpreter = Interpreter::with_init(settings, move |vm| {
            if let Err(err) = plugins.install_all(vm) {
                eprintln!("nanopython plugin install error: {err}");
                std::process::exit(2);
            }
        });

        let exit_code = interpreter.run(move |vm| run_mode(vm, mode));
        ExitCode::from(exit_code)
    }
}

fn parse_cli(args: Vec<String>) -> Result<(Settings, RunMode), ExitCode> {
    let mut iter = args.into_iter();
    let program = iter.next().unwrap_or_else(|| "nanopython".to_owned());
    let mut rest: Vec<String> = iter.collect();

    if rest.is_empty() {
        print_usage(&program);
        return Err(ExitCode::from(1));
    }

    if rest[0] == "-h" || rest[0] == "--help" {
        print_usage(&program);
        return Err(ExitCode::SUCCESS);
    }

    let mut settings = Settings::default();
    settings
        .path_list
        .push(rustpython_pylib::LIB_PATH.to_owned());

    append_env_paths("RUSTPYTHONPATH", &mut settings.path_list);
    append_env_paths("PYTHONPATH", &mut settings.path_list);

    let mode = if rest[0] == "-c" {
        if rest.len() < 2 {
            eprintln!("nanopython: -c requires a command string");
            return Err(ExitCode::from(2));
        }
        let command = rest.remove(1);
        rest.remove(0);
        settings.argv.push("-c".to_owned());
        settings.argv.extend(rest);
        RunMode::Command(command)
    } else if rest[0] == "-m" {
        if rest.len() < 2 {
            eprintln!("nanopython: -m requires a module name");
            return Err(ExitCode::from(2));
        }
        let module = rest.remove(1);
        rest.remove(0);
        settings.argv.push(module.clone());
        settings.argv.extend(rest);
        RunMode::Module(module)
    } else {
        let script = rest.remove(0);
        settings.argv.push(script.clone());
        settings.argv.extend(rest);
        RunMode::Script(script)
    };

    Ok((settings, mode))
}

fn append_env_paths(var_name: &str, out: &mut Vec<String>) {
    if let Some(raw) = env::var_os(var_name) {
        out.extend(
            env::split_paths(&raw)
                .map(PathBuf::into_os_string)
                .filter_map(|s| s.into_string().ok()),
        );
    }
}

fn print_usage(program: &str) {
    eprintln!(
        "Usage: {program} [-c CMD | -m MODULE | SCRIPT.py] [args...]\n\n\
         Notes:\n\
         - Uses RustPython VM backend.\n\
         - Loads stdlib from rustpython-pylib plus PYTHONPATH/RUSTPYTHONPATH."
    );
}

fn setup_main_module(vm: &VirtualMachine) -> PyResult<Scope> {
    let scope = vm.new_scope_with_builtins();
    let main_module = vm.new_module("__main__", scope.globals.clone(), None);
    main_module
        .dict()
        .set_item("__annotations__", vm.ctx.new_dict().into(), vm)
        .expect("failed to initialize __main__.__annotations__");

    vm.sys_module
        .get_attr("modules", vm)?
        .set_item("__main__", main_module.into(), vm)?;

    Ok(scope)
}

fn run_mode(vm: &VirtualMachine, mode: RunMode) -> PyResult<()> {
    let scope = setup_main_module(vm)?;

    if !vm.state.settings.safe_path {
        vm.run_code_string(
            vm.new_scope_with_builtins(),
            "import sys; sys.path.insert(0, '')",
            "<embedded>".to_owned(),
        )?;
    }

    if vm.state.settings.import_site {
        let _ = vm.import("site", 0);
    }

    match mode {
        RunMode::Command(command) => vm
            .run_code_string(scope, &command, "<stdin>".to_owned())
            .map(|_| ()),
        RunMode::Module(module) => vm.run_module(&module),
        RunMode::Script(script) => vm.run_script(scope, &script),
    }
}
