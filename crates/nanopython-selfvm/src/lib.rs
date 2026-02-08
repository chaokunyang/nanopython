use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use nanopython_core::{NanoPythonError, Result};
use nanopython_parser::{
    AssignTarget, BinaryOp, ComprehensionClause, ExceptHandler, Expr, ParsedModule, Stmt, UnaryOp,
    parse_expression, parse_source,
};

type EnvRef = Rc<RefCell<Env>>;
type ModuleRef = Rc<RefCell<HashMap<String, Value>>>;
type BuiltinFn = fn(&mut SelfVm, &[Value]) -> Result<Value>;
type BuiltinMethodFn = fn(&mut SelfVm, &Value, &[Value]) -> Result<Value>;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ValueKey {
    None,
    Bool(bool),
    Int(i64),
    Str(String),
    Bytes(Vec<u8>),
    Path(PathBuf),
    EnumMember {
        enum_name: String,
        name: String,
        value: i64,
    },
}

#[derive(Clone)]
enum Value {
    None,
    Bool(bool),
    Int(i64),
    Str(String),
    Bytes(Vec<u8>),
    List(Rc<RefCell<Vec<Value>>>),
    Dict(Rc<RefCell<BTreeMap<ValueKey, Value>>>),
    Set(Rc<RefCell<Vec<Value>>>),
    EnumMember {
        enum_name: String,
        name: String,
        value: i64,
    },
    EnumAuto,
    Function(Rc<UserFunction>),
    Lambda(Rc<LambdaFunction>),
    ClassMethod(Box<Value>),
    Property(Box<Value>),
    BuiltinFunction(BuiltinFn),
    BuiltinMethod {
        receiver: Box<Value>,
        func: BuiltinMethodFn,
    },
    BoundMethod {
        function: Rc<UserFunction>,
        receiver: Box<Value>,
    },
    Class(Rc<ClassDef>),
    Instance(Rc<RefCell<Instance>>),
    Range {
        start: i64,
        stop: i64,
        step: i64,
    },
    Generator(Rc<RefCell<GeneratorState>>),
    Module(ModuleRef),
    File(Rc<RefCell<FileState>>),
    Path(PathBuf),
    ArgParser(Rc<RefCell<ArgParserState>>),
    ArgGroup(Rc<RefCell<ArgParserState>>),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.repr())
    }
}

impl Value {
    fn repr(&self) -> String {
        match self {
            Self::None => "None".to_owned(),
            Self::Bool(v) => {
                if *v {
                    "True".to_owned()
                } else {
                    "False".to_owned()
                }
            }
            Self::Int(v) => v.to_string(),
            Self::Str(v) => v.clone(),
            Self::Bytes(v) => repr_bytes(v),
            Self::List(values) => {
                let parts: Vec<String> = values.borrow().iter().map(Value::repr).collect();
                format!("[{}]", parts.join(", "))
            }
            Self::Dict(values) => {
                let parts: Vec<String> = values
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.repr(), v.repr()))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            Self::Set(values) => {
                let parts: Vec<String> = values.borrow().iter().map(Value::repr).collect();
                format!("{{{}}}", parts.join(", "))
            }
            Self::EnumMember {
                enum_name, name, ..
            } => format!("{enum_name}.{name}"),
            Self::EnumAuto => "<enum.auto>".to_owned(),
            Self::Function(func) => format!("<function {}>", func.name),
            Self::Lambda(_) => "<lambda>".to_owned(),
            Self::ClassMethod(_) => "<classmethod>".to_owned(),
            Self::Property(_) => "<property>".to_owned(),
            Self::BuiltinFunction(_) => "<builtin function>".to_owned(),
            Self::BuiltinMethod { .. } => "<builtin method>".to_owned(),
            Self::BoundMethod { function, .. } => format!("<bound method {}>", function.name),
            Self::Class(class) => format!("<class {}>", class.name),
            Self::Instance(instance) => format!("<{} instance>", instance.borrow().class.name),
            Self::Range { start, stop, step } => {
                format!("range({start}, {stop}, {step})")
            }
            Self::Generator(_) => "<generator>".to_owned(),
            Self::Module(_) => "<module>".to_owned(),
            Self::File(state) => format!("<file {}>", state.borrow().path),
            Self::Path(path) => path.to_string_lossy().into_owned(),
            Self::ArgParser(_) => "<ArgumentParser>".to_owned(),
            Self::ArgGroup(_) => "<_MutuallyExclusiveGroup>".to_owned(),
        }
    }

    fn is_truthy(&self) -> bool {
        match self {
            Self::None => false,
            Self::Bool(v) => *v,
            Self::Int(v) => *v != 0,
            Self::Str(v) => !v.is_empty(),
            Self::Bytes(v) => !v.is_empty(),
            Self::List(v) => !v.borrow().is_empty(),
            Self::Dict(v) => !v.borrow().is_empty(),
            Self::Set(v) => !v.borrow().is_empty(),
            Self::Generator(v) => !v.borrow().is_exhausted(),
            _ => true,
        }
    }
}

impl ValueKey {
    fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::None => Ok(Self::None),
            Value::Bool(v) => Ok(Self::Bool(*v)),
            Value::Int(v) => Ok(Self::Int(*v)),
            Value::Str(v) => Ok(Self::Str(v.clone())),
            Value::Bytes(v) => Ok(Self::Bytes(v.clone())),
            Value::Path(v) => Ok(Self::Path(v.clone())),
            Value::EnumMember {
                enum_name, name, value
            } => Ok(Self::EnumMember {
                enum_name: enum_name.clone(),
                name: name.clone(),
                value: *value,
            }),
            _ => Err(runtime_err("unsupported key type")),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Self::None => Value::None,
            Self::Bool(v) => Value::Bool(*v),
            Self::Int(v) => Value::Int(*v),
            Self::Str(v) => Value::Str(v.clone()),
            Self::Bytes(v) => Value::Bytes(v.clone()),
            Self::Path(v) => Value::Path(v.clone()),
            Self::EnumMember {
                enum_name,
                name,
                value,
            } => Value::EnumMember {
                enum_name: enum_name.clone(),
                name: name.clone(),
                value: *value,
            },
        }
    }

    fn repr(&self) -> String {
        match self {
            Self::None => "None".to_owned(),
            Self::Bool(v) => {
                if *v {
                    "True".to_owned()
                } else {
                    "False".to_owned()
                }
            }
            Self::Int(v) => v.to_string(),
            Self::Str(v) => v.clone(),
            Self::Bytes(v) => repr_bytes(v),
            Self::Path(v) => v.to_string_lossy().into_owned(),
            Self::EnumMember {
                enum_name, name, ..
            } => format!("{enum_name}.{name}"),
        }
    }
}

#[derive(Clone)]
struct UserFunction {
    name: String,
    params: Vec<String>,
    param_defaults: Vec<Option<Expr>>,
    vararg: Option<String>,
    body: Vec<Stmt>,
    closure: EnvRef,
    is_generator: bool,
}

#[derive(Clone)]
struct LambdaFunction {
    params: Vec<String>,
    body: Expr,
    closure: EnvRef,
}

#[derive(Clone)]
struct ClassDef {
    name: String,
    attrs: HashMap<String, Value>,
    bases: Vec<Rc<ClassDef>>,
}

#[derive(Clone)]
struct Instance {
    class: Rc<ClassDef>,
    attrs: HashMap<String, Value>,
}

struct GeneratorState {
    values: Vec<Value>,
    index: usize,
}

impl GeneratorState {
    fn is_exhausted(&self) -> bool {
        self.index >= self.values.len()
    }

    fn next(&mut self) -> Option<Value> {
        if self.index >= self.values.len() {
            None
        } else {
            let value = self.values[self.index].clone();
            self.index += 1;
            Some(value)
        }
    }
}

struct FileState {
    path: String,
    file: File,
    closed: bool,
}

#[derive(Clone)]
struct ArgOption {
    flags: Vec<String>,
    dest: String,
    positional: bool,
    action: ArgAction,
    nargs_star: bool,
    arg_type: ArgType,
    default: Value,
}

#[derive(Clone, Copy)]
enum ArgAction {
    Store,
    StoreTrue,
    Append,
}

#[derive(Clone, Copy)]
enum ArgType {
    String,
    Path,
    Int,
}

struct ArgParserState {
    prog: String,
    description: String,
    options: Vec<ArgOption>,
}

#[derive(Clone)]
struct Env {
    parent: Option<EnvRef>,
    values: HashMap<String, Value>,
}

impl Env {
    fn new(parent: Option<EnvRef>) -> EnvRef {
        Rc::new(RefCell::new(Self {
            parent,
            values: HashMap::new(),
        }))
    }
}

fn env_get(env: &EnvRef, name: &str) -> Option<Value> {
    if let Some(value) = env.borrow().values.get(name) {
        return Some(value.clone());
    }

    let parent = env.borrow().parent.clone();
    parent.and_then(|parent| env_get(&parent, name))
}

fn env_set_local(env: &EnvRef, name: impl Into<String>, value: Value) {
    env.borrow_mut().values.insert(name.into(), value);
}

enum Flow {
    Normal,
    Return(Value),
    Break,
    Continue,
}

#[derive(Default)]
struct ExecState {
    yields: Option<Vec<Value>>,
}

pub struct SelfVm {
    module_paths: Vec<PathBuf>,
    modules: HashMap<String, Value>,
    argv: Vec<String>,
}

impl Default for SelfVm {
    fn default() -> Self {
        let mut paths = Vec::new();
        if let Ok(cwd) = env::current_dir() {
            paths.push(cwd);
        }

        if let Some(py_path) = env::var_os("PYTHONPATH") {
            for path in env::split_paths(&py_path) {
                if !path.as_os_str().is_empty() {
                    paths.push(path);
                }
            }
        }

        Self {
            module_paths: paths,
            modules: HashMap::new(),
            argv: env::args().collect(),
        }
    }
}

impl SelfVm {
    pub fn run_script(&mut self, script_path: &str) -> ExitCode {
        match fs::read_to_string(script_path) {
            Ok(source) => {
                let filename = Path::new(script_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(script_path);
                match self.execute_source(&source, filename) {
                    Ok(_) => ExitCode::SUCCESS,
                    Err(err) => {
                        eprintln!("nanopython-self runtime error: {err}");
                        ExitCode::from(1)
                    }
                }
            }
            Err(err) => {
                eprintln!("nanopython-self io error: {err}");
                ExitCode::from(1)
            }
        }
    }

    pub fn run_command(&mut self, source: &str) -> ExitCode {
        match self.execute_source(source, "<stdin>") {
            Ok(_) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("nanopython-self runtime error: {err}");
                ExitCode::from(1)
            }
        }
    }

    pub fn execute_source(&mut self, source: &str, _filename: &str) -> Result<()> {
        let parsed = parse_source(source)?;
        self.execute_parsed_with_name(parsed, "__main__")
    }

    pub fn execute_parsed(&mut self, module: ParsedModule) -> Result<()> {
        self.execute_parsed_with_name(module, "__main__")
    }

    pub fn run_module(&mut self, module_name: &str, module_args: &[String]) -> ExitCode {
        self.argv.clear();
        self.argv.push(module_name.to_owned());
        self.argv.extend(module_args.iter().cloned());
        self.modules.remove("sys");

        let mut path = match self.resolve_module_path(module_name) {
            Some(path) => path,
            None => {
                eprintln!(
                    "nanopython-self runtime error: {}",
                    runtime_err(&format!("module `{module_name}` not found"))
                );
                return ExitCode::from(1);
            }
        };
        if path.file_name().and_then(|name| name.to_str()) == Some("__init__.py") {
            if let Some(main_path) = self.resolve_module_path(&format!("{module_name}.__main__")) {
                path = main_path;
            }
        }

        let result = fs::read_to_string(&path)
            .map_err(|err| NanoPythonError::Io(format!("cannot read {}: {err}", path.display())))
            .and_then(|source| parse_source(&source))
            .and_then(|parsed| self.execute_parsed_with_name(parsed, "__main__"));

        match result {
            Ok(_) => ExitCode::SUCCESS,
            Err(NanoPythonError::Runtime(message)) if message.starts_with("SystemExit") => {
                parse_system_exit_code(&message)
            }
            Err(err) => {
                eprintln!("nanopython-self runtime error: {err}");
                ExitCode::from(1)
            }
        }
    }

    fn execute_parsed_with_name(&mut self, module: ParsedModule, module_name: &str) -> Result<()> {
        let env = self.new_global_env();
        env_set_local(&env, "__name__", Value::Str(module_name.to_owned()));
        let mut state = ExecState::default();
        match self.exec_block(&module.body, &env, &mut state)? {
            Flow::Return(_) => Ok(()),
            _ => Ok(()),
        }
    }

    fn new_global_env(&self) -> EnvRef {
        let env = Env::new(None);
        env_set_local(&env, "print", Value::BuiltinFunction(Self::builtin_print));
        env_set_local(&env, "range", Value::BuiltinFunction(Self::builtin_range));
        env_set_local(&env, "len", Value::BuiltinFunction(Self::builtin_len));
        env_set_local(&env, "repr", Value::BuiltinFunction(Self::builtin_repr));
        env_set_local(&env, "list", Value::BuiltinFunction(Self::builtin_list));
        env_set_local(&env, "dict", Value::BuiltinFunction(Self::builtin_dict));
        env_set_local(&env, "set", Value::BuiltinFunction(Self::builtin_set));
        env_set_local(&env, "open", Value::BuiltinFunction(Self::builtin_open));
        env_set_local(&env, "str", Value::BuiltinFunction(Self::builtin_str));
        env_set_local(&env, "int", Value::BuiltinFunction(Self::builtin_int));
        env_set_local(&env, "bool", Value::BuiltinFunction(Self::builtin_bool));
        env_set_local(&env, "sorted", Value::BuiltinFunction(Self::builtin_sorted));
        env_set_local(
            &env,
            "enumerate",
            Value::BuiltinFunction(Self::builtin_enumerate),
        );
        env_set_local(
            &env,
            "isinstance",
            Value::BuiltinFunction(Self::builtin_isinstance),
        );
        env_set_local(
            &env,
            "getattr",
            Value::BuiltinFunction(Self::builtin_getattr),
        );
        env_set_local(
            &env,
            "hasattr",
            Value::BuiltinFunction(Self::builtin_hasattr),
        );
        env_set_local(&env, "any", Value::BuiltinFunction(Self::builtin_any));
        env_set_local(&env, "all", Value::BuiltinFunction(Self::builtin_all));
        env_set_local(&env, "zip", Value::BuiltinFunction(Self::builtin_zip));
        env_set_local(
            &env,
            "reversed",
            Value::BuiltinFunction(Self::builtin_reversed),
        );
        env_set_local(&env, "next", Value::BuiltinFunction(Self::builtin_next));
        env_set_local(
            &env,
            "classmethod",
            Value::BuiltinFunction(Self::builtin_classmethod),
        );
        env_set_local(&env, "property", Value::BuiltinFunction(Self::builtin_property));
        self.install_builtin_exception(&env, "object");
        self.install_builtin_exception(&env, "Exception");
        self.install_builtin_exception(&env, "RuntimeError");
        self.install_builtin_exception(&env, "AssertionError");
        self.install_builtin_exception(&env, "OSError");
        self.install_builtin_exception(&env, "ValueError");
        self.install_builtin_exception(&env, "ImportError");
        env
    }

    fn install_builtin_exception(&self, env: &EnvRef, name: &str) {
        env_set_local(
            env,
            name.to_owned(),
            Value::Class(Rc::new(ClassDef {
                name: name.to_owned(),
                attrs: HashMap::new(),
                bases: Vec::new(),
            })),
        );
    }

    fn exec_block(&mut self, block: &[Stmt], env: &EnvRef, state: &mut ExecState) -> Result<Flow> {
        for stmt in block {
            let flow = self
                .exec_stmt(stmt, env, state)
                .map_err(|err| add_stmt_context(err, stmt))?;
            if !matches!(flow, Flow::Normal) {
                return Ok(flow);
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_stmt(&mut self, stmt: &Stmt, env: &EnvRef, state: &mut ExecState) -> Result<Flow> {
        match stmt {
            Stmt::Assign { target, value } => {
                let value = self.eval_expr(value, env, state)?;
                self.assign_target(target, value, env, state)?;
                Ok(Flow::Normal)
            }
            Stmt::AugAssign { target, op, value } => {
                let left = self.read_target_value(target, env, state)?;
                let right = self.eval_expr(value, env, state)?;
                let value = self.eval_binary(*op, left, right)?;
                self.assign_target(target, value, env, state)?;
                Ok(Flow::Normal)
            }
            Stmt::Expr(expr) => {
                let _ = self.eval_expr(expr, env, state)?;
                Ok(Flow::Normal)
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                let cond = self.eval_expr(condition, env, state)?;
                if cond.is_truthy() {
                    self.exec_block(then_body, env, state)
                } else {
                    self.exec_block(else_body, env, state)
                }
            }
            Stmt::While { condition, body } => {
                loop {
                    let cond = self.eval_expr(condition, env, state)?;
                    if !cond.is_truthy() {
                        break;
                    }

                    match self.exec_block(body, env, state)? {
                        Flow::Normal => {}
                        Flow::Continue => continue,
                        Flow::Break => break,
                        flow @ Flow::Return(_) => return Ok(flow),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::For {
                target,
                iterable,
                body,
            } => {
                let iterable = self.eval_expr(iterable, env, state)?;
                let values = self.iterable_to_values(&iterable)?;
                for item in values {
                    self.assign_target(target, item, env, state)?;
                    match self.exec_block(body, env, state)? {
                        Flow::Normal => {}
                        Flow::Continue => continue,
                        Flow::Break => break,
                        flow @ Flow::Return(_) => return Ok(flow),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Def {
                name,
                params,
                defaults,
                vararg,
                decorators,
                body,
                is_generator,
            } => {
                let mut value = Value::Function(Rc::new(UserFunction {
                    name: name.clone(),
                    params: params.clone(),
                    param_defaults: defaults.clone(),
                    vararg: vararg.clone(),
                    body: body.clone(),
                    closure: env.clone(),
                    is_generator: *is_generator,
                }));

                for decorator in decorators.iter().rev() {
                    let decorator_value = self.eval_expr(decorator, env, state)?;
                    value = self.call_value(decorator_value, &[value], state)?;
                }

                env_set_local(env, name, value);
                Ok(Flow::Normal)
            }
            Stmt::Class {
                name,
                bases,
                decorators,
                body,
            } => {
                let class_env = Env::new(Some(env.clone()));
                let mut class_state = ExecState::default();
                self.exec_block(body, &class_env, &mut class_state)?;

                let mut attrs = class_env.borrow().values.clone();
                let declared = body
                    .iter()
                    .filter_map(|stmt| match stmt {
                        Stmt::Assign {
                            target: AssignTarget::Name(name),
                            ..
                        } => Some(Value::Str(name.clone())),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if !declared.is_empty() {
                    attrs.insert(
                        "__decl_order__".to_owned(),
                        Value::List(Rc::new(RefCell::new(declared))),
                    );
                }
                let mut resolved_bases = Vec::new();
                for base in bases {
                    let value = self.eval_expr(base, env, state)?;
                    match value {
                        Value::Class(class) => resolved_bases.push(class),
                        _ => return Err(runtime_err("class base must be a class")),
                    }
                }
                let is_enum_class = resolved_bases
                    .iter()
                    .any(|base| base.name == "Enum" || base.name == "IntEnum");
                if is_enum_class {
                    let mut next_value = 1i64;
                    for stmt in body {
                        let Stmt::Assign {
                            target: AssignTarget::Name(member_name),
                            ..
                        } = stmt
                        else {
                            continue;
                        };
                        if member_name.starts_with('_') {
                            continue;
                        }
                        let Some(raw_value) = attrs.get(member_name).cloned() else {
                            continue;
                        };
                        let value = match raw_value {
                            Value::EnumAuto => {
                                let value = next_value;
                                next_value += 1;
                                value
                            }
                            Value::Int(value) => {
                                next_value = value + 1;
                                value
                            }
                            _ => continue,
                        };
                        attrs.insert(
                            member_name.clone(),
                            Value::EnumMember {
                                enum_name: name.clone(),
                                name: member_name.clone(),
                                value,
                            },
                        );
                    }
                }
                let class = Value::Class(Rc::new(ClassDef {
                    name: name.clone(),
                    attrs,
                    bases: resolved_bases,
                }));

                let mut class_value = class;
                for decorator in decorators.iter().rev() {
                    let decorator_value = self.eval_expr(decorator, env, state)?;
                    class_value = self.call_value(decorator_value, &[class_value], state)?;
                }

                env_set_local(env, name, class_value);
                Ok(Flow::Normal)
            }
            Stmt::With { expr, alias, body } => {
                let manager = self.eval_expr(expr, env, state)?;
                let entered = match self.get_attr(&manager, "__enter__") {
                    Ok(enter) => self.call_value(enter, &[], state)?,
                    Err(_) => manager.clone(),
                };

                if let Some(alias) = alias {
                    env_set_local(env, alias.clone(), entered);
                }

                let flow = self.exec_block(body, env, state)?;
                if let Ok(exit) = self.get_attr(&manager, "__exit__") {
                    let _ = self.call_value(exit, &[Value::None, Value::None, Value::None], state);
                }

                Ok(flow)
            }
            Stmt::Try {
                body,
                handlers,
                finally_body,
            } => {
                let mut flow = Flow::Normal;
                let mut pending_error = None;
                match self.exec_block(body, env, state) {
                    Ok(inner_flow) => flow = inner_flow,
                    Err(err) => {
                        if let Some(handler_flow) =
                            self.try_handle_exception(&err, handlers, env, state)?
                        {
                            flow = handler_flow;
                        } else {
                            pending_error = Some(err);
                        }
                    }
                }
                if !finally_body.is_empty() {
                    match self.exec_block(finally_body, env, state) {
                        Ok(final_flow) => {
                            if !matches!(final_flow, Flow::Normal) {
                                return Ok(final_flow);
                            }
                        }
                        Err(err) => return Err(err),
                    }
                }
                if let Some(err) = pending_error {
                    Err(err)
                } else {
                    Ok(flow)
                }
            }
            Stmt::Return(value) => {
                let value = match value {
                    Some(expr) => self.eval_expr(expr, env, state)?,
                    None => Value::None,
                };
                Ok(Flow::Return(value))
            }
            Stmt::Yield(value) => {
                let value = match value {
                    Some(expr) => self.eval_expr(expr, env, state)?,
                    None => Value::None,
                };
                if let Some(yields) = &mut state.yields {
                    yields.push(value);
                    Ok(Flow::Normal)
                } else {
                    Err(runtime_err("yield outside generator function"))
                }
            }
            Stmt::Raise(value) => {
                let (name, message) = match value {
                    Some(expr) => {
                        let value = self.eval_expr(expr, env, state)?;
                        exception_from_value(&value)
                    }
                    None => ("RuntimeError".to_owned(), String::new()),
                };
                let text = if message.is_empty() {
                    name
                } else {
                    format!("{name}: {message}")
                };
                Err(NanoPythonError::Runtime(text))
            }
            Stmt::Assert { condition, message } => {
                let cond = self.eval_expr(condition, env, state)?;
                if cond.is_truthy() {
                    return Ok(Flow::Normal);
                }
                let text = if let Some(message) = message {
                    self.eval_expr(message, env, state)?.repr()
                } else {
                    String::new()
                };
                let err = if text.is_empty() {
                    "AssertionError".to_owned()
                } else {
                    format!("AssertionError: {text}")
                };
                Err(NanoPythonError::Runtime(err))
            }
            Stmt::Import { module } => {
                let imported = self.load_module(module)?;
                let root_name = module.split('.').next().unwrap_or(module);
                env_set_local(env, root_name.to_owned(), imported);
                Ok(Flow::Normal)
            }
            Stmt::FromImport { module, names } => {
                let imported = self.load_module(module)?;
                for imported_name in names {
                    if imported_name.name == "*" {
                        let Value::Module(module_map) = &imported else {
                            return Err(runtime_err("star import requires module value"));
                        };
                        for (name, value) in module_map.borrow().iter() {
                            if !name.starts_with('_') {
                                env_set_local(env, name.clone(), value.clone());
                            }
                        }
                        continue;
                    }
                    let value = self.get_attr(&imported, &imported_name.name)?;
                    let local_name = imported_name
                        .alias
                        .as_ref()
                        .unwrap_or(&imported_name.name)
                        .clone();
                    env_set_local(env, local_name, value);
                }
                Ok(Flow::Normal)
            }
            Stmt::Pass => Ok(Flow::Normal),
            Stmt::Break => Ok(Flow::Break),
            Stmt::Continue => Ok(Flow::Continue),
        }
    }

    fn try_handle_exception(
        &mut self,
        err: &NanoPythonError,
        handlers: &[ExceptHandler],
        env: &EnvRef,
        state: &mut ExecState,
    ) -> Result<Option<Flow>> {
        let (err_name, err_message) = error_kind_and_message(&err);
        for handler in handlers {
            let matches = if let Some(exc_expr) = &handler.exception {
                let expected = self.eval_expr(exc_expr, env, state)?;
                exception_matches(&expected, &err_name)
            } else {
                true
            };
            if !matches {
                continue;
            }

            if let Some(alias) = &handler.alias {
                let value = if err_message.is_empty() {
                    Value::Str(err_name.clone())
                } else {
                    Value::Str(err_message.clone())
                };
                env_set_local(env, alias.clone(), value);
            }
            let flow = self.exec_block(&handler.body, env, state)?;
            return Ok(Some(flow));
        }
        Ok(None)
    }

    fn read_target_value(
        &mut self,
        target: &AssignTarget,
        env: &EnvRef,
        state: &mut ExecState,
    ) -> Result<Value> {
        match target {
            AssignTarget::Name(name) => {
                env_get(env, name).ok_or_else(|| runtime_err("name used before assignment"))
            }
            AssignTarget::Attr { object, name } => {
                let object = self.eval_expr(object, env, state)?;
                self.get_attr(&object, name)
            }
            AssignTarget::Index { object, index } => {
                let object = self.eval_expr(object, env, state)?;
                let index = self.eval_expr(index, env, state)?;
                self.get_index(&object, &index)
            }
            AssignTarget::Slice {
                object,
                start,
                end,
                step,
            } => {
                let object = self.eval_expr(object, env, state)?;
                let start = self.eval_optional_int(start, env, state)?;
                let end = self.eval_optional_int(end, env, state)?;
                let step = self.eval_optional_int(step, env, state)?;
                self.get_slice(&object, start, end, step)
            }
            AssignTarget::Tuple(_) => Err(runtime_err("tuple target is not readable")),
        }
    }

    fn assign_target(
        &mut self,
        target: &AssignTarget,
        value: Value,
        env: &EnvRef,
        state: &mut ExecState,
    ) -> Result<()> {
        match target {
            AssignTarget::Name(name) => {
                env_set_local(env, name.clone(), value);
                Ok(())
            }
            AssignTarget::Attr { object, name } => {
                let object = self.eval_expr(object, env, state)?;
                match object {
                    Value::Instance(instance) => {
                        instance.borrow_mut().attrs.insert(name.clone(), value);
                        Ok(())
                    }
                    Value::Module(module) => {
                        module.borrow_mut().insert(name.clone(), value);
                        Ok(())
                    }
                    _ => Err(runtime_err(
                        "attribute assignment target must be instance/module",
                    )),
                }
            }
            AssignTarget::Index { object, index } => {
                let object = self.eval_expr(object, env, state)?;
                let index = self.eval_expr(index, env, state)?;
                match object {
                    Value::List(list) => {
                        let raw = expect_int(&index)?;
                        let mut list = list.borrow_mut();
                        let len = list.len() as i64;
                        let idx = if raw < 0 { len + raw } else { raw };
                        if idx < 0 || idx >= len {
                            return Err(runtime_err("list assignment index out of range"));
                        }
                        list[idx as usize] = value;
                        Ok(())
                    }
                    Value::Dict(map) => {
                        let key = key_of(&index)?;
                        map.borrow_mut().insert(key, value);
                        Ok(())
                    }
                    _ => Err(runtime_err("index assignment target must be list/dict")),
                }
            }
            AssignTarget::Slice {
                object,
                start,
                end,
                step,
            } => {
                let object = self.eval_expr(object, env, state)?;
                let start = self.eval_optional_int(start, env, state)?;
                let end = self.eval_optional_int(end, env, state)?;
                let step = self.eval_optional_int(step, env, state)?.unwrap_or(1);
                if step != 1 {
                    return Err(runtime_err("slice assignment only supports step=1"));
                }
                let replacement = self.iterable_to_values(&value)?;
                match object {
                    Value::List(list) => {
                        let mut values = list.borrow_mut();
                        let len = values.len();
                        let (start, end) = normalize_slice_bounds(len, start, end);
                        values.splice(start..end, replacement);
                        Ok(())
                    }
                    _ => Err(runtime_err("slice assignment requires list target")),
                }
            }
            AssignTarget::Tuple(targets) => {
                let items = self.iterable_to_values(&value)?;
                if items.len() != targets.len() {
                    return Err(runtime_err("unpack assignment arity mismatch"));
                }
                for (target, item) in targets.iter().zip(items) {
                    self.assign_target(target, item, env, state)?;
                }
                Ok(())
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr, env: &EnvRef, state: &mut ExecState) -> Result<Value> {
        match expr {
            Expr::Name(name) => env_get(env, name)
                .ok_or_else(|| runtime_err(&format!("name `{name}` is not defined"))),
            Expr::Int(v) => Ok(Value::Int(*v)),
            Expr::Bool(v) => Ok(Value::Bool(*v)),
            Expr::None => Ok(Value::None),
            Expr::Str(v) => Ok(Value::Str(v.clone())),
            Expr::FString(template) => Ok(Value::Str(self.eval_fstring(template, env, state)?)),
            Expr::List(items) => {
                let mut out = Vec::new();
                for item in items {
                    if let Expr::Starred(inner) = item {
                        let value = self.eval_expr(inner, env, state)?;
                        out.extend(self.iterable_to_values(&value)?);
                    } else {
                        out.push(self.eval_expr(item, env, state)?);
                    }
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            Expr::Dict(items) => {
                let mut out = BTreeMap::new();
                for (key, value) in items {
                    let key = self.eval_expr(key, env, state)?;
                    let value = self.eval_expr(value, env, state)?;
                    out.insert(key_of(&key)?, value);
                }
                Ok(Value::Dict(Rc::new(RefCell::new(out))))
            }
            Expr::Set(items) => {
                if !items.is_empty() && items.iter().all(|item| matches!(item, Expr::Starred(_))) {
                    let mut merged = BTreeMap::new();
                    let mut all_dicts = true;
                    for item in items {
                        let Expr::Starred(inner) = item else {
                            continue;
                        };
                        let value = self.eval_expr(inner, env, state)?;
                        let Value::Dict(values) = value else {
                            all_dicts = false;
                            break;
                        };
                        for (key, value) in values.borrow().iter() {
                            merged.insert(key.clone(), value.clone());
                        }
                    }
                    if all_dicts {
                        return Ok(Value::Dict(Rc::new(RefCell::new(merged))));
                    }
                }

                let mut out = Vec::new();
                for item in items {
                    if let Expr::Starred(inner) = item {
                        let value = self.eval_expr(inner, env, state)?;
                        for expanded in self.iterable_to_values(&value)? {
                            if !contains_value(&out, &expanded) {
                                out.push(expanded);
                            }
                        }
                    } else {
                        let value = self.eval_expr(item, env, state)?;
                        if !contains_value(&out, &value) {
                            out.push(value);
                        }
                    }
                }
                Ok(Value::Set(Rc::new(RefCell::new(out))))
            }
            Expr::Tuple(items) => {
                let mut out = Vec::new();
                for item in items {
                    if let Expr::Starred(inner) = item {
                        let value = self.eval_expr(inner, env, state)?;
                        out.extend(self.iterable_to_values(&value)?);
                    } else {
                        out.push(self.eval_expr(item, env, state)?);
                    }
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            Expr::ListComp { element, clauses } => {
                let envs = self.eval_comprehension_envs(clauses, env, state)?;
                let mut out = Vec::with_capacity(envs.len());
                for local in envs {
                    out.push(self.eval_expr(element, &local, state)?);
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            Expr::DictComp {
                key,
                value,
                clauses,
            } => {
                let envs = self.eval_comprehension_envs(clauses, env, state)?;
                let mut out = BTreeMap::new();
                for local in envs {
                    let key = self.eval_expr(key, &local, state)?;
                    let value = self.eval_expr(value, &local, state)?;
                    out.insert(key_of(&key)?, value);
                }
                Ok(Value::Dict(Rc::new(RefCell::new(out))))
            }
            Expr::SetComp { element, clauses } => {
                let envs = self.eval_comprehension_envs(clauses, env, state)?;
                let mut out = Vec::new();
                for local in envs {
                    let value = self.eval_expr(element, &local, state)?;
                    if !contains_value(&out, &value) {
                        out.push(value);
                    }
                }
                Ok(Value::Set(Rc::new(RefCell::new(out))))
            }
            Expr::GeneratorComp { element, clauses } => {
                let envs = self.eval_comprehension_envs(clauses, env, state)?;
                let mut values = Vec::with_capacity(envs.len());
                for local in envs {
                    values.push(self.eval_expr(element, &local, state)?);
                }
                Ok(Value::Generator(Rc::new(RefCell::new(GeneratorState {
                    values,
                    index: 0,
                }))))
            }
            Expr::IfExpr {
                condition,
                then_expr,
                else_expr,
            } => {
                let cond = self.eval_expr(condition, env, state)?;
                if cond.is_truthy() {
                    self.eval_expr(then_expr, env, state)
                } else {
                    self.eval_expr(else_expr, env, state)
                }
            }
            Expr::Lambda { params, body } => Ok(Value::Lambda(Rc::new(LambdaFunction {
                params: params.clone(),
                body: (**body).clone(),
                closure: env.clone(),
            }))),
            Expr::Unary { op, expr } => {
                let value = self.eval_expr(expr, env, state)?;
                match op {
                    UnaryOp::Neg => Ok(Value::Int(-expect_int(&value)?)),
                    UnaryOp::Not => Ok(Value::Bool(!value.is_truthy())),
                }
            }
            Expr::Binary { op, left, right } => {
                if *op == BinaryOp::And {
                    let left = self.eval_expr(left, env, state)?;
                    if !left.is_truthy() {
                        return Ok(left);
                    }
                    return self.eval_expr(right, env, state);
                }
                if *op == BinaryOp::Or {
                    let left = self.eval_expr(left, env, state)?;
                    if left.is_truthy() {
                        return Ok(left);
                    }
                    return self.eval_expr(right, env, state);
                }

                let left = self.eval_expr(left, env, state)?;
                let right = self.eval_expr(right, env, state)?;
                self.eval_binary(*op, left, right)
            }
            Expr::Call { func, args } => {
                let function = self.eval_expr(func, env, state)?;
                let mut positional = Vec::new();
                let mut keyword = Vec::new();
                let mut flat_args = Vec::new();
                for arg in args {
                    if arg.name.is_none() {
                        if let Expr::Starred(inner) = &arg.value {
                            let value = self.eval_expr(inner, env, state)?;
                            let expanded = self.iterable_to_values(&value)?;
                            for item in expanded {
                                flat_args.push(item.clone());
                                positional.push(item);
                            }
                        } else {
                            let value = self.eval_expr(&arg.value, env, state)?;
                            flat_args.push(value.clone());
                            positional.push(value);
                        }
                    } else {
                        let value = self.eval_expr(&arg.value, env, state)?;
                        flat_args.push(value.clone());
                        keyword.push((arg.name.clone().unwrap_or_default(), value));
                    }
                }
                match function {
                    Value::Function(function) => {
                        self.call_user_function_with_kwargs(function, None, &positional, &keyword)
                    }
                    Value::BoundMethod { function, receiver } => self.call_user_function_with_kwargs(
                        function,
                        Some(*receiver),
                        &positional,
                        &keyword,
                    ),
                    Value::Lambda(lambda) => {
                        self.call_lambda_function_with_kwargs(lambda, &positional, &keyword)
                    }
                    Value::Class(class) => {
                        self.instantiate_class_with_kwargs(class, &positional, &keyword, state)
                    }
                    other => self.call_value(other, &flat_args, state),
                }
            }
            Expr::Attr { object, name } => {
                let object = self.eval_expr(object, env, state)?;
                self.get_attr(&object, name)
            }
            Expr::Index { object, index } => {
                let object = self.eval_expr(object, env, state)?;
                let index = self.eval_expr(index, env, state)?;
                self.get_index(&object, &index)
            }
            Expr::Slice {
                object,
                start,
                end,
                step,
            } => {
                let object = self.eval_expr(object, env, state)?;
                let start = start
                    .as_ref()
                    .map(|expr| self.eval_expr(expr, env, state).and_then(|v| expect_int(&v)))
                    .transpose()?;
                let end = end
                    .as_ref()
                    .map(|expr| self.eval_expr(expr, env, state).and_then(|v| expect_int(&v)))
                    .transpose()?;
                let step = step
                    .as_ref()
                    .map(|expr| self.eval_expr(expr, env, state).and_then(|v| expect_int(&v)))
                    .transpose()?;
                self.get_slice(&object, start, end, step)
            }
            Expr::Starred(inner) => self.eval_expr(inner, env, state),
        }
    }

    fn eval_comprehension_envs(
        &mut self,
        clauses: &[ComprehensionClause],
        env: &EnvRef,
        state: &mut ExecState,
    ) -> Result<Vec<EnvRef>> {
        let mut envs = vec![Env::new(Some(env.clone()))];
        for clause in clauses {
            let mut next = Vec::new();
            for current in envs {
                let iterable = self.eval_expr(&clause.iterable, &current, state)?;
                let values = self.iterable_to_values(&iterable)?;
                for item in values {
                    let local = Env::new(Some(current.clone()));
                    self.assign_target(&clause.target, item, &local, state)?;
                    let mut matched = true;
                    for condition in &clause.conditions {
                        let value = self.eval_expr(condition, &local, state)?;
                        if !value.is_truthy() {
                            matched = false;
                            break;
                        }
                    }
                    if matched {
                        next.push(local);
                    }
                }
            }
            envs = next;
        }
        Ok(envs)
    }

    fn eval_fstring(&mut self, template: &str, env: &EnvRef, state: &mut ExecState) -> Result<String> {
        let chars: Vec<char> = template.chars().collect();
        let mut i = 0usize;
        let mut out = String::new();
        while i < chars.len() {
            let ch = chars[i];
            if ch == '{' {
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    out.push('{');
                    i += 2;
                    continue;
                }
                i += 1;
                let mut depth = 0i32;
                let mut field = String::new();
                let mut in_single = false;
                let mut in_double = false;
                let mut escaped = false;
                while i < chars.len() {
                    let cur = chars[i];
                    if escaped {
                        field.push(cur);
                        escaped = false;
                        i += 1;
                        continue;
                    }
                    if (in_single || in_double) && cur == '\\' {
                        field.push(cur);
                        escaped = true;
                        i += 1;
                        continue;
                    }
                    if !in_double && cur == '\'' {
                        in_single = !in_single;
                        field.push(cur);
                        i += 1;
                        continue;
                    }
                    if !in_single && cur == '"' {
                        in_double = !in_double;
                        field.push(cur);
                        i += 1;
                        continue;
                    }
                    if !in_single && !in_double {
                        if cur == '{' {
                            depth += 1;
                            field.push(cur);
                            i += 1;
                            continue;
                        }
                        if cur == '}' {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                            field.push(cur);
                            i += 1;
                            continue;
                        }
                    }
                    field.push(cur);
                    i += 1;
                }
                if i >= chars.len() || chars[i] != '}' {
                    return Err(runtime_err("unterminated f-string expression"));
                }
                i += 1;
                let (expr_src, repr_mode) = split_fstring_field(&field);
                let parsed = parse_expression(expr_src.trim(), 1)?;
                let value = self.eval_expr(&parsed, env, state)?;
                let rendered = if repr_mode {
                    value.repr()
                } else {
                    value.repr()
                };
                out.push_str(&rendered);
                continue;
            }
            if ch == '}' {
                if i + 1 < chars.len() && chars[i + 1] == '}' {
                    out.push('}');
                    i += 2;
                    continue;
                }
                return Err(runtime_err("single `}` in f-string"));
            }
            out.push(ch);
            i += 1;
        }
        Ok(out)
    }

    fn eval_optional_int(
        &mut self,
        value: &Option<Expr>,
        env: &EnvRef,
        state: &mut ExecState,
    ) -> Result<Option<i64>> {
        value
            .as_ref()
            .map(|expr| self.eval_expr(expr, env, state).and_then(|v| expect_int(&v)))
            .transpose()
    }

    fn eval_binary(&self, op: BinaryOp, left: Value, right: Value) -> Result<Value> {
        match op {
            BinaryOp::Add => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{a}{b}"))),
                (Value::List(a), Value::List(b)) => {
                    let mut values = a.borrow().clone();
                    values.extend(b.borrow().iter().cloned());
                    Ok(Value::List(Rc::new(RefCell::new(values))))
                }
                _ => Err(runtime_err("unsupported operands for +")),
            },
            BinaryOp::Sub => Ok(Value::Int(expect_int(&left)? - expect_int(&right)?)),
            BinaryOp::Mul => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                (Value::Str(text), Value::Int(times)) | (Value::Int(times), Value::Str(text)) => {
                    let repeat = if times <= 0 { 0 } else { times as usize };
                    Ok(Value::Str(text.repeat(repeat)))
                }
                (Value::List(values), Value::Int(times))
                | (Value::Int(times), Value::List(values)) => {
                    let repeat = if times <= 0 { 0 } else { times as usize };
                    let original = values.borrow();
                    let mut out = Vec::with_capacity(original.len() * repeat);
                    for _ in 0..repeat {
                        out.extend(original.iter().cloned());
                    }
                    Ok(Value::List(Rc::new(RefCell::new(out))))
                }
                _ => Err(runtime_err("unsupported operands for *")),
            },
            BinaryOp::Div => match (left, right) {
                (Value::Path(path), Value::Str(part)) => Ok(Value::Path(path.join(part))),
                (Value::Path(path), Value::Path(part)) => Ok(Value::Path(path.join(part))),
                (left, right) => {
                    let rhs = expect_int(&right)?;
                    if rhs == 0 {
                        return Err(runtime_err("division by zero"));
                    }
                    Ok(Value::Int(expect_int(&left)? / rhs))
                }
            },
            BinaryOp::FloorDiv => {
                let rhs = expect_int(&right)?;
                if rhs == 0 {
                    return Err(runtime_err("division by zero"));
                }
                Ok(Value::Int(expect_int(&left)? / rhs))
            }
            BinaryOp::Mod => {
                let rhs = expect_int(&right)?;
                if rhs == 0 {
                    return Err(runtime_err("modulo by zero"));
                }
                Ok(Value::Int(expect_int(&left)? % rhs))
            }
            BinaryOp::Eq => Ok(Value::Bool(value_eq(&left, &right))),
            BinaryOp::Ne => Ok(Value::Bool(!value_eq(&left, &right))),
            BinaryOp::Lt => Ok(Value::Bool(expect_int(&left)? < expect_int(&right)?)),
            BinaryOp::Le => Ok(Value::Bool(expect_int(&left)? <= expect_int(&right)?)),
            BinaryOp::Gt => Ok(Value::Bool(expect_int(&left)? > expect_int(&right)?)),
            BinaryOp::Ge => Ok(Value::Bool(expect_int(&left)? >= expect_int(&right)?)),
            BinaryOp::BitOr => Ok(Value::Int(expect_int(&left)? | expect_int(&right)?)),
            BinaryOp::BitXor => Ok(Value::Int(expect_int(&left)? ^ expect_int(&right)?)),
            BinaryOp::BitAnd => Ok(Value::Int(expect_int(&left)? & expect_int(&right)?)),
            BinaryOp::LShift => Ok(Value::Int(expect_int(&left)? << expect_int(&right)?)),
            BinaryOp::RShift => Ok(Value::Int(expect_int(&left)? >> expect_int(&right)?)),
            BinaryOp::In => Ok(Value::Bool(value_in(&left, &right)?)),
            BinaryOp::NotIn => Ok(Value::Bool(!value_in(&left, &right)?)),
            BinaryOp::Is => Ok(Value::Bool(value_is(&left, &right))),
            BinaryOp::IsNot => Ok(Value::Bool(!value_is(&left, &right))),
            BinaryOp::And | BinaryOp::Or => Err(runtime_err("internal short-circuit error")),
        }
    }

    fn call_value(
        &mut self,
        function: Value,
        args: &[Value],
        state: &mut ExecState,
    ) -> Result<Value> {
        match function {
            Value::BuiltinFunction(func) => func(self, args),
            Value::BuiltinMethod { receiver, func } => func(self, &receiver, args),
            Value::Function(function) => self.call_user_function(function, None, args),
            Value::Lambda(lambda) => self.call_lambda_function(lambda, args),
            Value::BoundMethod { function, receiver } => {
                self.call_user_function(function, Some(*receiver), args)
            }
            Value::Class(class) => self.instantiate_class(class, args, state),
            _ => Err(runtime_err(&format!("value `{}` is not callable", function.repr()))),
        }
    }

    fn call_user_function(
        &mut self,
        function: Rc<UserFunction>,
        receiver: Option<Value>,
        args: &[Value],
    ) -> Result<Value> {
        self.call_user_function_with_kwargs(function, receiver, args, &[])
    }

    fn call_user_function_with_kwargs(
        &mut self,
        function: Rc<UserFunction>,
        receiver: Option<Value>,
        args: &[Value],
        kwargs: &[(String, Value)],
    ) -> Result<Value> {
        let local = Env::new(Some(function.closure.clone()));
        let mut param_start = 0usize;
        let mut next_arg = 0usize;
        let mut kw_values: HashMap<String, Value> = kwargs.iter().cloned().collect();

        if let Some(receiver) = receiver {
            if function.params.is_empty() {
                return Err(runtime_err("method missing self parameter"));
            }
            env_set_local(&local, function.params[0].clone(), receiver);
            param_start = 1;
        }

        let mut default_state = ExecState::default();
        for (param_index, param) in function.params.iter().enumerate().skip(param_start) {
            if let Some(value) = args.get(next_arg) {
                env_set_local(&local, param.clone(), value.clone());
                next_arg += 1;
            } else if let Some(value) = kw_values.remove(param) {
                env_set_local(&local, param.clone(), value);
            } else if let Some(Some(default_expr)) = function.param_defaults.get(param_index) {
                let default_value =
                    self.eval_expr(default_expr, &function.closure, &mut default_state)?;
                env_set_local(&local, param.clone(), default_value);
            } else {
                env_set_local(&local, param.clone(), Value::None);
            }
        }
        if let Some(vararg) = &function.vararg {
            let rest = if next_arg < args.len() {
                args[next_arg..].to_vec()
            } else {
                Vec::new()
            };
            env_set_local(&local, vararg.clone(), Value::List(Rc::new(RefCell::new(rest))));
        }

        if function.is_generator {
            let mut state = ExecState {
                yields: Some(Vec::new()),
            };
            let _ = self.exec_block(&function.body, &local, &mut state)?;
            let values = state.yields.unwrap_or_default();
            return Ok(Value::Generator(Rc::new(RefCell::new(GeneratorState {
                values,
                index: 0,
            }))));
        }

        let mut state = ExecState::default();
        match self.exec_block(&function.body, &local, &mut state)? {
            Flow::Return(value) => Ok(value),
            _ => Ok(Value::None),
        }
    }

    fn call_lambda_function(&mut self, lambda: Rc<LambdaFunction>, args: &[Value]) -> Result<Value> {
        self.call_lambda_function_with_kwargs(lambda, args, &[])
    }

    fn call_lambda_function_with_kwargs(
        &mut self,
        lambda: Rc<LambdaFunction>,
        args: &[Value],
        kwargs: &[(String, Value)],
    ) -> Result<Value> {
        let local = Env::new(Some(lambda.closure.clone()));
        let mut kw_values: HashMap<String, Value> = kwargs.iter().cloned().collect();
        let mut next_arg = 0usize;
        for param in &lambda.params {
            let value = if let Some(value) = args.get(next_arg) {
                next_arg += 1;
                value.clone()
            } else if let Some(value) = kw_values.remove(param) {
                value
            } else {
                Value::None
            };
            env_set_local(&local, param.clone(), value);
        }
        let mut state = ExecState::default();
        self.eval_expr(&lambda.body, &local, &mut state)
    }

    fn instantiate_class(
        &mut self,
        class: Rc<ClassDef>,
        args: &[Value],
        state: &mut ExecState,
    ) -> Result<Value> {
        self.instantiate_class_with_kwargs(class, args, &[], state)
    }

    fn instantiate_class_with_kwargs(
        &mut self,
        class: Rc<ClassDef>,
        args: &[Value],
        kwargs: &[(String, Value)],
        state: &mut ExecState,
    ) -> Result<Value> {
        let instance = Value::Instance(Rc::new(RefCell::new(Instance {
            class: class.clone(),
            attrs: HashMap::new(),
        })));

        if let Some(init) = Self::class_lookup_attr(&class, "__init__") {
            match init {
                Value::Function(function) => {
                    let _ = self.call_user_function_with_kwargs(
                        function.clone(),
                        Some(instance.clone()),
                        args,
                        kwargs,
                    )?;
                }
                other => {
                    let mut flat = args.to_vec();
                    flat.extend(kwargs.iter().map(|(_, value)| value.clone()));
                    let _ = self.call_value(other.clone(), &flat, state)?;
                }
            }
        } else if let Some(Value::List(fields)) = Self::class_lookup_attr(&class, "__dataclass_fields__") {
            let fields = fields.borrow().clone();
            let mut kw_values: HashMap<String, Value> = kwargs.iter().cloned().collect();
            if args.len() + kw_values.len() > fields.len() {
                return Err(runtime_err("too many arguments for dataclass constructor"));
            }
            let mut instance_ref = match &instance {
                Value::Instance(instance_ref) => instance_ref.borrow_mut(),
                _ => unreachable!(),
            };
            for (index, field) in fields.iter().enumerate() {
                let Value::Str(name) = field else {
                    continue;
                };
                let value = if let Some(arg) = args.get(index) {
                    arg.clone()
                } else if let Some(value) = kw_values.remove(name) {
                    value
                } else {
                    class
                        .attrs
                        .get(name)
                        .map(deep_clone_value)
                        .unwrap_or(Value::None)
                };
                instance_ref.attrs.insert(name.clone(), value);
            }
        }

        Ok(instance)
    }

    fn class_lookup_attr(class: &Rc<ClassDef>, name: &str) -> Option<Value> {
        if let Some(value) = class.attrs.get(name) {
            return Some(value.clone());
        }
        for base in &class.bases {
            if let Some(value) = Self::class_lookup_attr(base, name) {
                return Some(value);
            }
        }
        None
    }

    fn class_is_subclass(child: &Rc<ClassDef>, parent: &Rc<ClassDef>) -> bool {
        if Rc::ptr_eq(child, parent) {
            return true;
        }
        for base in &child.bases {
            if Self::class_is_subclass(base, parent) {
                return true;
            }
        }
        false
    }

    fn value_is_instance_of(&self, value: &Value, class_value: &Value) -> bool {
        match class_value {
            Value::Class(parent) => match value {
                Value::Instance(instance) => {
                    let class = instance.borrow().class.clone();
                    Self::class_is_subclass(&class, parent)
                }
                Value::Class(class) => Self::class_is_subclass(class, parent),
                _ => false,
            },
            Value::List(values) => values
                .borrow()
                .iter()
                .any(|candidate| self.value_is_instance_of(value, candidate)),
            Value::BuiltinFunction(func) => {
                if (*func as usize) == (Self::builtin_int as usize) {
                    return matches!(value, Value::Int(_));
                }
                if (*func as usize) == (Self::builtin_str as usize) {
                    return matches!(value, Value::Str(_));
                }
                if (*func as usize) == (Self::builtin_bool as usize) {
                    return matches!(value, Value::Bool(_));
                }
                if (*func as usize) == (Self::builtin_list as usize) {
                    return matches!(value, Value::List(_));
                }
                if (*func as usize) == (Self::builtin_dict as usize) {
                    return matches!(value, Value::Dict(_));
                }
                if (*func as usize) == (Self::builtin_set as usize) {
                    return matches!(value, Value::Set(_));
                }
                false
            }
            _ => false,
        }
    }

    fn get_attr(&mut self, object: &Value, name: &str) -> Result<Value> {
        match object {
            Value::BuiltinFunction(func) => {
                if (*func as usize) == (Self::builtin_int as usize) && name == "from_bytes" {
                    return Ok(Value::BuiltinMethod {
                        receiver: Box::new(object.clone()),
                        func: Self::builtin_int_from_bytes,
                    });
                }
                Err(runtime_err("value has no attributes"))
            }
            Value::EnumMember {
                name: member_name,
                value: member_value,
                ..
            } => match name {
                "name" => Ok(Value::Str(member_name.clone())),
                "value" => Ok(Value::Int(*member_value)),
                _ => Err(runtime_err(&format!("enum attribute `{name}` not found"))),
            },
            Value::Instance(instance) => {
                if let Some(value) = instance.borrow().attrs.get(name) {
                    return Ok(value.clone());
                }
                if let Some(value) = Self::class_lookup_attr(&instance.borrow().class, name) {
                    let resolved = match value {
                        Value::Function(function) => Value::BoundMethod {
                            function: function.clone(),
                            receiver: Box::new(object.clone()),
                        },
                        Value::ClassMethod(inner) => match inner.as_ref() {
                            Value::Function(function) => Value::BoundMethod {
                                function: function.clone(),
                                receiver: Box::new(Value::Class(instance.borrow().class.clone())),
                            },
                            other => other.clone(),
                        },
                        Value::Property(inner) => match inner.as_ref() {
                            Value::Function(function) => self.call_user_function(
                                function.clone(),
                                Some(object.clone()),
                                &[],
                            )?,
                            _ => return Err(runtime_err("property getter must be function")),
                        },
                        other => other.clone(),
                    };
                    return Ok(resolved);
                }
                Err(runtime_err(&format!("attribute `{name}` not found")))
            }
            Value::Class(class) => {
                let Some(value) = Self::class_lookup_attr(class, name) else {
                    return Err(runtime_err(&format!("class attribute `{name}` not found")));
                };
                Ok(match &value {
                    Value::ClassMethod(inner) => match inner.as_ref() {
                        Value::Function(function) => Value::BoundMethod {
                            function: function.clone(),
                            receiver: Box::new(Value::Class(class.clone())),
                        },
                        other => other.clone(),
                    },
                    _ => value,
                })
            }
            Value::Module(module) => module
                .borrow()
                .get(name)
                .cloned()
                .ok_or_else(|| runtime_err(&format!("module attribute `{name}` not found"))),
            Value::File(_) => self.file_attr(object, name),
            Value::Path(_) => self.path_attr(object, name),
            Value::ArgParser(_) | Value::ArgGroup(_) => self.argparse_attr(object, name),
            Value::Str(_) => self.str_attr(object, name),
            Value::List(_) => self.list_attr(object, name),
            Value::Dict(_) => self.dict_attr(object, name),
            Value::Set(_) => self.set_attr(object, name),
            _ => Err(runtime_err("value has no attributes")),
        }
    }

    fn str_attr(&self, object: &Value, name: &str) -> Result<Value> {
        match name {
            "split" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_split,
            }),
            "strip" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_strip,
            }),
            "lstrip" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_lstrip,
            }),
            "rstrip" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_rstrip,
            }),
            "lower" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_lower,
            }),
            "upper" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_upper,
            }),
            "startswith" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_startswith,
            }),
            "endswith" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_endswith,
            }),
            "isupper" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_isupper,
            }),
            "islower" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_islower,
            }),
            "isalpha" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_isalpha,
            }),
            "isalnum" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_isalnum,
            }),
            "isdigit" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_isdigit,
            }),
            "capitalize" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_capitalize,
            }),
            "replace" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_replace,
            }),
            "join" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_join,
            }),
            "encode" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_str_encode,
            }),
            _ => Err(runtime_err("unknown str method")),
        }
    }

    fn list_attr(&self, object: &Value, name: &str) -> Result<Value> {
        match name {
            "append" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_list_append,
            }),
            "extend" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_list_extend,
            }),
            "pop" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_list_pop,
            }),
            "sort" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_list_sort,
            }),
            _ => Err(runtime_err("unknown list method")),
        }
    }

    fn dict_attr(&self, object: &Value, name: &str) -> Result<Value> {
        match name {
            "get" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_dict_get,
            }),
            "keys" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_dict_keys,
            }),
            "values" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_dict_values,
            }),
            "items" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_dict_items,
            }),
            "setdefault" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_dict_setdefault,
            }),
            _ => Err(runtime_err("unknown dict method")),
        }
    }

    fn set_attr(&self, object: &Value, name: &str) -> Result<Value> {
        match name {
            "add" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_set_add,
            }),
            "remove" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_set_remove,
            }),
            "discard" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_set_discard,
            }),
            "copy" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_set_copy,
            }),
            _ => Err(runtime_err(&format!("unknown set method `{name}`"))),
        }
    }

    fn file_attr(&self, object: &Value, name: &str) -> Result<Value> {
        match name {
            "read" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_file_read,
            }),
            "write" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_file_write,
            }),
            "close" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_file_close,
            }),
            "__enter__" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_file_enter,
            }),
            "__exit__" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_file_exit,
            }),
            _ => Err(runtime_err("unknown file method")),
        }
    }

    fn path_attr(&self, object: &Value, name: &str) -> Result<Value> {
        let Value::Path(path) = object else {
            return Err(runtime_err("path attribute receiver must be path"));
        };
        match name {
            "resolve" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_path_resolve,
            }),
            "exists" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_path_exists,
            }),
            "is_file" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_path_is_file,
            }),
            "is_dir" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_path_is_dir,
            }),
            "read_text" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_path_read_text,
            }),
            "write_text" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_path_write_text,
            }),
            "open" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_path_open,
            }),
            "relative_to" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_path_relative_to,
            }),
            "mkdir" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_path_mkdir,
            }),
            "parent" => Ok(Value::Path(path.parent().unwrap_or(path).to_path_buf())),
            "parents" => {
                let mut out = Vec::new();
                let mut cur = path.parent();
                while let Some(parent) = cur {
                    out.push(Value::Path(parent.to_path_buf()));
                    cur = parent.parent();
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            "name" => Ok(Value::Str(
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_owned(),
            )),
            "stem" => Ok(Value::Str(
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_owned(),
            )),
            "suffix" => Ok(Value::Str(
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| format!(".{ext}"))
                    .unwrap_or_default(),
            )),
            _ => Err(runtime_err("unknown path attribute")),
        }
    }

    fn argparse_attr(&self, object: &Value, name: &str) -> Result<Value> {
        match name {
            "add_argument" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_argparse_add_argument,
            }),
            "parse_args" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_argparse_parse_args,
            }),
            "add_mutually_exclusive_group" => Ok(Value::BuiltinMethod {
                receiver: Box::new(object.clone()),
                func: Self::builtin_argparse_add_group,
            }),
            _ => Err(runtime_err("unknown argparse attribute")),
        }
    }

    fn get_index(&self, object: &Value, index: &Value) -> Result<Value> {
        match object {
            Value::List(values) => {
                let raw = expect_int(index)?;
                let len = values.borrow().len() as i64;
                let idx = if raw < 0 { len + raw } else { raw };
                if idx < 0 || idx >= len {
                    return Err(runtime_err("list index out of range"));
                }
                values
                    .borrow()
                    .get(idx as usize)
                    .cloned()
                    .ok_or_else(|| runtime_err("list index out of range"))
            }
            Value::Dict(values) => {
                let key = key_of(index)?;
                values
                    .borrow()
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| runtime_err("dict key not found"))
            }
            Value::Str(value) => {
                let raw = expect_int(index)?;
                let len = value.chars().count() as i64;
                let idx = if raw < 0 { len + raw } else { raw };
                if idx < 0 || idx >= len {
                    return Err(runtime_err("string index out of range"));
                }
                let ch = value
                    .chars()
                    .nth(idx as usize)
                    .ok_or_else(|| runtime_err("string index out of range"))?;
                Ok(Value::Str(ch.to_string()))
            }
            Value::Bytes(bytes) => {
                let raw = expect_int(index)?;
                let len = bytes.len() as i64;
                let idx = if raw < 0 { len + raw } else { raw };
                if idx < 0 || idx >= len {
                    return Err(runtime_err("bytes index out of range"));
                }
                Ok(Value::Int(bytes[idx as usize] as i64))
            }
            Value::Class(class) => Ok(Value::Class(class.clone())),
            _ => Err(runtime_err(
                "indexing is only supported on list/dict/str/bytes/class",
            )),
        }
    }

    fn get_slice(
        &self,
        object: &Value,
        start: Option<i64>,
        end: Option<i64>,
        step: Option<i64>,
    ) -> Result<Value> {
        let step = step.unwrap_or(1);
        if step == 0 {
            return Err(runtime_err("slice step cannot be zero"));
        }
        match object {
            Value::List(values) => {
                let values = values.borrow();
                let indices = slice_indices(values.len(), start, end, step);
                let mut out = Vec::with_capacity(indices.len());
                for idx in indices {
                    out.push(values[idx].clone());
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            Value::Str(value) => {
                let chars: Vec<char> = value.chars().collect();
                let indices = slice_indices(chars.len(), start, end, step);
                let mut out = String::new();
                for idx in indices {
                    out.push(chars[idx]);
                }
                Ok(Value::Str(out))
            }
            Value::Bytes(bytes) => {
                let indices = slice_indices(bytes.len(), start, end, step);
                let mut out = Vec::with_capacity(indices.len());
                for idx in indices {
                    out.push(bytes[idx]);
                }
                Ok(Value::Bytes(out))
            }
            _ => Err(runtime_err("slicing is only supported on list/str/bytes")),
        }
    }

    fn iterable_to_values(&self, iterable: &Value) -> Result<Vec<Value>> {
        match iterable {
            Value::Str(text) => Ok(text
                .chars()
                .map(|ch| Value::Str(ch.to_string()))
                .collect::<Vec<_>>()),
            Value::List(values) => Ok(values.borrow().clone()),
            Value::Set(values) => Ok(values.borrow().clone()),
            Value::Dict(values) => Ok(values
                .borrow()
                .keys()
                .map(ValueKey::to_value)
                .collect()),
            Value::Bytes(bytes) => Ok(bytes.iter().map(|byte| Value::Int(*byte as i64)).collect()),
            Value::Range { start, stop, step } => {
                if *step == 0 {
                    return Err(runtime_err("range step cannot be zero"));
                }
                let mut out = Vec::new();
                let mut cur = *start;
                if *step > 0 {
                    while cur < *stop {
                        out.push(Value::Int(cur));
                        cur += *step;
                    }
                } else {
                    while cur > *stop {
                        out.push(Value::Int(cur));
                        cur += *step;
                    }
                }
                Ok(out)
            }
            Value::Generator(generator) => {
                let mut out = Vec::new();
                while let Some(value) = generator.borrow_mut().next() {
                    out.push(value);
                }
                Ok(out)
            }
            _ => Err(runtime_err("value is not iterable")),
        }
    }

    fn load_module(&mut self, module_name: &str) -> Result<Value> {
        if is_blocked_module(module_name) {
            return Err(runtime_err(&format!(
                "module '{module_name}' is disabled in nanopython min profile"
            )));
        }

        if let Some(value) = self.modules.get(module_name) {
            return Ok(value.clone());
        }

        if let Some(module) = self.builtin_module(module_name) {
            self.modules.insert(module_name.to_owned(), module.clone());
            return Ok(module);
        }

        let path = self
            .resolve_module_path(module_name)
            .ok_or_else(|| runtime_err(&format!("module `{module_name}` not found")))?;
        let source = fs::read_to_string(&path)
            .map_err(|err| NanoPythonError::Io(format!("cannot read {}: {err}", path.display())))?;

        let parsed = parse_source(&source)?;
        let module_env = self.new_global_env();
        env_set_local(&module_env, "__name__", Value::Str(module_name.to_owned()));
        let mut state = ExecState::default();
        let _ = self.exec_block(&parsed.body, &module_env, &mut state)?;

        let module_map = Rc::new(RefCell::new(module_env.borrow().values.clone()));
        let module_value = Value::Module(module_map);
        self.modules
            .insert(module_name.to_owned(), module_value.clone());
        Ok(module_value)
    }

    fn resolve_module_path(&self, module_name: &str) -> Option<PathBuf> {
        let relative = module_name.replace('.', "/");
        for base in &self.module_paths {
            let module_file = base.join(format!("{relative}.py"));
            if module_file.exists() {
                return Some(module_file);
            }

            let package_init = base.join(relative.clone()).join("__init__.py");
            if package_init.exists() {
                return Some(package_init);
            }
        }
        None
    }

    fn builtin_module(&self, module_name: &str) -> Option<Value> {
        match module_name {
            "dataclasses" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module.borrow_mut().insert(
                    "dataclass".to_owned(),
                    Value::BuiltinFunction(Self::builtin_dataclass),
                );
                module.borrow_mut().insert(
                    "field".to_owned(),
                    Value::BuiltinFunction(Self::builtin_field),
                );
                Some(Value::Module(module))
            }
            "enum" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module.borrow_mut().insert(
                    "auto".to_owned(),
                    Value::BuiltinFunction(Self::builtin_enum_auto),
                );
                module.borrow_mut().insert(
                    "Enum".to_owned(),
                    Value::Class(Rc::new(ClassDef {
                        name: "Enum".to_owned(),
                        attrs: HashMap::new(),
                        bases: Vec::new(),
                    })),
                );
                module.borrow_mut().insert(
                    "IntEnum".to_owned(),
                    Value::Class(Rc::new(ClassDef {
                        name: "IntEnum".to_owned(),
                        attrs: HashMap::new(),
                        bases: Vec::new(),
                    })),
                );
                Some(Value::Module(module))
            }
            "copy" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module.borrow_mut().insert(
                    "deepcopy".to_owned(),
                    Value::BuiltinFunction(Self::builtin_copy_deepcopy),
                );
                Some(Value::Module(module))
            }
            "os" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module.borrow_mut().insert(
                    "remove".to_owned(),
                    Value::BuiltinFunction(Self::builtin_os_remove),
                );
                Some(Value::Module(module))
            }
            "gc" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module.borrow_mut().insert(
                    "collect".to_owned(),
                    Value::BuiltinFunction(Self::builtin_gc_collect),
                );
                module.borrow_mut().insert(
                    "get_threshold".to_owned(),
                    Value::BuiltinFunction(Self::builtin_gc_get_threshold),
                );
                Some(Value::Module(module))
            }
            "typing" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                for name in [
                    "Any", "Dict", "List", "Optional", "Set", "Tuple", "Union", "Iterable",
                    "Callable", "Type",
                ] {
                    module.borrow_mut().insert(
                        name.to_owned(),
                        Value::Class(Rc::new(ClassDef {
                            name: name.to_owned(),
                            attrs: HashMap::new(),
                            bases: Vec::new(),
                        })),
                    );
                }
                Some(Value::Module(module))
            }
            "__future__" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module
                    .borrow_mut()
                    .insert("annotations".to_owned(), Value::None);
                Some(Value::Module(module))
            }
            "abc" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module.borrow_mut().insert(
                    "ABC".to_owned(),
                    Value::Class(Rc::new(ClassDef {
                        name: "ABC".to_owned(),
                        attrs: HashMap::new(),
                        bases: Vec::new(),
                    })),
                );
                module.borrow_mut().insert(
                    "abstractmethod".to_owned(),
                    Value::BuiltinFunction(Self::builtin_identity_decorator),
                );
                Some(Value::Module(module))
            }
            "pathlib" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module.borrow_mut().insert(
                    "Path".to_owned(),
                    Value::BuiltinFunction(Self::builtin_path_ctor),
                );
                Some(Value::Module(module))
            }
            "warnings" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module.borrow_mut().insert(
                    "warn".to_owned(),
                    Value::BuiltinFunction(Self::builtin_warnings_warn),
                );
                Some(Value::Module(module))
            }
            "sys" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                let argv = self.argv.iter().cloned().map(Value::Str).collect::<Vec<_>>();
                module.borrow_mut().insert(
                    "argv".to_owned(),
                    Value::List(Rc::new(RefCell::new(argv))),
                );
                module.borrow_mut().insert("stderr".to_owned(), Value::None);
                module.borrow_mut().insert("stdout".to_owned(), Value::None);
                module.borrow_mut().insert(
                    "exit".to_owned(),
                    Value::BuiltinFunction(Self::builtin_sys_exit),
                );
                Some(Value::Module(module))
            }
            "argparse" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module.borrow_mut().insert(
                    "ArgumentParser".to_owned(),
                    Value::BuiltinFunction(Self::builtin_argparse_argument_parser),
                );
                module.borrow_mut().insert(
                    "Namespace".to_owned(),
                    Value::Class(Rc::new(ClassDef {
                        name: "Namespace".to_owned(),
                        attrs: HashMap::new(),
                        bases: Vec::new(),
                    })),
                );
                Some(Value::Module(module))
            }
            "keyword" => {
                let module = Rc::new(RefCell::new(HashMap::new()));
                module.borrow_mut().insert(
                    "iskeyword".to_owned(),
                    Value::BuiltinFunction(Self::builtin_keyword_iskeyword),
                );
                Some(Value::Module(module))
            }
            _ => None,
        }
    }

    fn builtin_print(&mut self, args: &[Value]) -> Result<Value> {
        let parts: Vec<String> = args.iter().map(Value::repr).collect();
        println!("{}", parts.join(" "));
        Ok(Value::None)
    }

    fn builtin_len(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Err(runtime_err("len() expects 1 argument"));
        };

        let size = match value {
            Value::Str(v) => v.chars().count() as i64,
            Value::Bytes(v) => v.len() as i64,
            Value::List(v) => v.borrow().len() as i64,
            Value::Dict(v) => v.borrow().len() as i64,
            Value::Set(v) => v.borrow().len() as i64,
            Value::Range { start, stop, step } => {
                if *step == 0 {
                    return Err(runtime_err("range step cannot be zero"));
                }
                (((stop - start) as f64) / (*step as f64)).max(0.0).ceil() as i64
            }
            _ => return Err(runtime_err("len() unsupported for this value")),
        };

        Ok(Value::Int(size))
    }

    fn builtin_range(&mut self, args: &[Value]) -> Result<Value> {
        let (start, stop, step) = match args {
            [stop] => (0, expect_int(stop)?, 1),
            [start, stop] => (expect_int(start)?, expect_int(stop)?, 1),
            [start, stop, step] => (expect_int(start)?, expect_int(stop)?, expect_int(step)?),
            _ => return Err(runtime_err("range() expects 1-3 integer arguments")),
        };

        if step == 0 {
            return Err(runtime_err("range() step cannot be zero"));
        }

        Ok(Value::Range { start, stop, step })
    }

    fn builtin_list(&mut self, args: &[Value]) -> Result<Value> {
        if args.is_empty() {
            return Ok(Value::List(Rc::new(RefCell::new(Vec::new()))));
        }
        if args.len() != 1 {
            return Err(runtime_err("list() expects at most 1 argument"));
        }
        let values = self.iterable_to_values(&args[0])?;
        Ok(Value::List(Rc::new(RefCell::new(values))))
    }

    fn builtin_dict(&mut self, args: &[Value]) -> Result<Value> {
        if args.is_empty() {
            return Ok(Value::Dict(Rc::new(RefCell::new(BTreeMap::new()))));
        }
        if args.len() != 1 {
            return Err(runtime_err("dict() expects at most 1 argument"));
        }
        match &args[0] {
            Value::Dict(values) => Ok(Value::Dict(Rc::new(RefCell::new(values.borrow().clone())))),
            Value::List(items) => {
                let mut out = BTreeMap::new();
                for item in items.borrow().iter() {
                    let pair = self.iterable_to_values(item)?;
                    if pair.len() != 2 {
                        return Err(runtime_err("dict() pair must have length 2"));
                    }
                    out.insert(key_of(&pair[0])?, pair[1].clone());
                }
                Ok(Value::Dict(Rc::new(RefCell::new(out))))
            }
            _ => Err(runtime_err("dict() unsupported iterable input")),
        }
    }

    fn builtin_set(&mut self, args: &[Value]) -> Result<Value> {
        if args.is_empty() {
            return Ok(Value::Set(Rc::new(RefCell::new(Vec::new()))));
        }
        if args.len() != 1 {
            return Err(runtime_err("set() expects at most 1 argument"));
        }
        let mut out = Vec::new();
        for value in self.iterable_to_values(&args[0])? {
            if !contains_value(&out, &value) {
                out.push(value);
            }
        }
        Ok(Value::Set(Rc::new(RefCell::new(out))))
    }

    fn builtin_open(&mut self, args: &[Value]) -> Result<Value> {
        let Some(path) = args.first() else {
            return Err(runtime_err("open() expects a path"));
        };

        let mode = if let Some(mode) = args.get(1) {
            expect_str(mode)?
        } else {
            "r".to_owned()
        };

        let path = expect_str(path)?;
        let file = match mode.as_str() {
            "r" => OpenOptions::new().read(true).open(&path),
            "w" => OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&path),
            "a" => OpenOptions::new().create(true).append(true).open(&path),
            _ => {
                return Err(runtime_err(
                    "open() mode only supports r/w/a in custom backend",
                ));
            }
        }
        .map_err(|err| NanoPythonError::Io(format!("open({path}) failed: {err}")))?;

        Ok(Value::File(Rc::new(RefCell::new(FileState {
            path,
            file,
            closed: false,
        }))))
    }

    fn builtin_path_ctor(&mut self, args: &[Value]) -> Result<Value> {
        if args.is_empty() {
            return Ok(Value::Path(PathBuf::from(".")));
        }
        Ok(Value::Path(expect_path(&args[0])?))
    }

    fn builtin_str(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Ok(Value::Str(String::new()));
        };
        Ok(Value::Str(value.repr()))
    }

    fn builtin_repr(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Err(runtime_err("repr() expects 1 argument"));
        };
        Ok(Value::Str(py_repr(value)))
    }

    fn builtin_str_split(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.split receiver must be string"));
        };
        let parts = if let Some(sep) = args.first() {
            let sep = expect_str(sep)?;
            if sep.is_empty() {
                return Err(runtime_err("empty separator"));
            }
            text.split(&sep).map(|part| Value::Str(part.to_owned())).collect()
        } else {
            text.split_whitespace()
                .map(|part| Value::Str(part.to_owned()))
                .collect()
        };
        Ok(Value::List(Rc::new(RefCell::new(parts))))
    }

    fn builtin_str_strip(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.strip receiver must be string"));
        };
        if let Some(chars) = args.first() {
            let chars = expect_str(chars)?;
            let trimmed = text.trim_matches(|ch| chars.contains(ch)).to_owned();
            return Ok(Value::Str(trimmed));
        }
        Ok(Value::Str(text.trim().to_owned()))
    }

    fn builtin_str_lstrip(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.lstrip receiver must be string"));
        };
        if let Some(chars) = args.first() {
            let chars = expect_str(chars)?;
            let trimmed = text
                .trim_start_matches(|ch| chars.contains(ch))
                .to_owned();
            return Ok(Value::Str(trimmed));
        }
        Ok(Value::Str(text.trim_start().to_owned()))
    }

    fn builtin_str_rstrip(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.rstrip receiver must be string"));
        };
        if let Some(chars) = args.first() {
            let chars = expect_str(chars)?;
            let trimmed = text.trim_end_matches(|ch| chars.contains(ch)).to_owned();
            return Ok(Value::Str(trimmed));
        }
        Ok(Value::Str(text.trim_end().to_owned()))
    }

    fn builtin_str_lower(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.lower receiver must be string"));
        };
        Ok(Value::Str(text.to_lowercase()))
    }

    fn builtin_str_upper(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.upper receiver must be string"));
        };
        Ok(Value::Str(text.to_uppercase()))
    }

    fn builtin_str_startswith(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.startswith receiver must be string"));
        };
        let Some(prefix) = args.first() else {
            return Err(runtime_err("str.startswith expects prefix"));
        };
        let prefix = expect_str(prefix)?;
        Ok(Value::Bool(text.starts_with(&prefix)))
    }

    fn builtin_str_endswith(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.endswith receiver must be string"));
        };
        let Some(suffix) = args.first() else {
            return Err(runtime_err("str.endswith expects suffix"));
        };
        let suffix = expect_str(suffix)?;
        Ok(Value::Bool(text.ends_with(&suffix)))
    }

    fn builtin_str_isupper(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.isupper receiver must be string"));
        };
        let mut has_alpha = false;
        for ch in text.chars() {
            if ch.is_alphabetic() {
                has_alpha = true;
                if !ch.is_uppercase() {
                    return Ok(Value::Bool(false));
                }
            }
        }
        Ok(Value::Bool(has_alpha))
    }

    fn builtin_str_islower(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.islower receiver must be string"));
        };
        let mut has_alpha = false;
        for ch in text.chars() {
            if ch.is_alphabetic() {
                has_alpha = true;
                if !ch.is_lowercase() {
                    return Ok(Value::Bool(false));
                }
            }
        }
        Ok(Value::Bool(has_alpha))
    }

    fn builtin_str_isalpha(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.isalpha receiver must be string"));
        };
        Ok(Value::Bool(
            !text.is_empty() && text.chars().all(|ch| ch.is_alphabetic()),
        ))
    }

    fn builtin_str_isalnum(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.isalnum receiver must be string"));
        };
        Ok(Value::Bool(
            !text.is_empty() && text.chars().all(|ch| ch.is_alphanumeric()),
        ))
    }

    fn builtin_str_isdigit(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.isdigit receiver must be string"));
        };
        Ok(Value::Bool(
            !text.is_empty() && text.chars().all(|ch| ch.is_ascii_digit()),
        ))
    }

    fn builtin_str_capitalize(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.capitalize receiver must be string"));
        };
        let mut chars = text.chars();
        let Some(first) = chars.next() else {
            return Ok(Value::Str(String::new()));
        };
        let mut out = first.to_uppercase().to_string();
        out.push_str(&chars.as_str().to_lowercase());
        Ok(Value::Str(out))
    }

    fn builtin_str_replace(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.replace receiver must be string"));
        };
        let Some(old) = args.first() else {
            return Err(runtime_err("str.replace expects old value"));
        };
        let Some(new) = args.get(1) else {
            return Err(runtime_err("str.replace expects new value"));
        };
        let old = expect_str(old)?;
        let new = expect_str(new)?;
        Ok(Value::Str(text.replace(&old, &new)))
    }

    fn builtin_str_join(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Str(sep) = receiver else {
            return Err(runtime_err("str.join receiver must be string"));
        };
        let Some(iterable) = args.first() else {
            return Err(runtime_err("str.join expects iterable"));
        };
        let mut parts = Vec::new();
        for value in self.iterable_to_values(iterable)? {
            parts.push(expect_str(&value)?);
        }
        Ok(Value::Str(parts.join(sep)))
    }

    fn builtin_str_encode(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Str(text) = receiver else {
            return Err(runtime_err("str.encode receiver must be string"));
        };
        let encoding = args
            .first()
            .map(expect_str)
            .transpose()?
            .unwrap_or_else(|| "utf-8".to_owned())
            .to_ascii_lowercase();
        if encoding != "utf-8" && encoding != "utf8" && encoding != "ascii" {
            return Err(runtime_err("str.encode only supports utf-8/ascii"));
        }
        if encoding == "ascii" && !text.is_ascii() {
            return Err(runtime_err("ascii codec can't encode non-ascii characters"));
        }
        Ok(Value::Bytes(text.as_bytes().to_vec()))
    }

    fn builtin_int(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Ok(Value::Int(0));
        };

        match value {
            Value::Int(v) => Ok(Value::Int(*v)),
            Value::Bool(v) => Ok(Value::Int(if *v { 1 } else { 0 })),
            Value::Str(v) => v
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| runtime_err("int() invalid string")),
            _ => Err(runtime_err("int() unsupported for this value")),
        }
    }

    fn builtin_int_from_bytes(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::BuiltinFunction(func) = receiver else {
            return Err(runtime_err("int.from_bytes receiver must be int"));
        };
        if (*func as usize) != (Self::builtin_int as usize) {
            return Err(runtime_err("int.from_bytes receiver must be int"));
        }
        let Some(data) = args.first() else {
            return Err(runtime_err("int.from_bytes expects bytes-like value"));
        };
        let byteorder = args
            .get(1)
            .map(expect_str)
            .transpose()?
            .unwrap_or_else(|| "big".to_owned())
            .to_ascii_lowercase();
        let signed = args.get(2).map(expect_bool).transpose()?.unwrap_or(false);
        let bytes = match data {
            Value::Bytes(bytes) => bytes.clone(),
            Value::List(values) => {
                let mut out = Vec::with_capacity(values.borrow().len());
                for value in values.borrow().iter() {
                    let byte = expect_int(value)?;
                    if !(0..=255).contains(&byte) {
                        return Err(runtime_err("int.from_bytes list item out of range"));
                    }
                    out.push(byte as u8);
                }
                out
            }
            _ => return Err(runtime_err("int.from_bytes expects bytes-like value")),
        };
        if bytes.len() > 8 {
            return Err(runtime_err("int.from_bytes supports at most 8 bytes"));
        }
        let bit_width = bytes.len() * 8;

        let mut acc = 0u64;
        match byteorder.as_str() {
            "little" => {
                for (idx, byte) in bytes.iter().enumerate() {
                    acc |= (*byte as u64) << (idx * 8);
                }
            }
            "big" => {
                for byte in bytes.iter() {
                    acc = (acc << 8) | *byte as u64;
                }
            }
            _ => return Err(runtime_err("byteorder must be either 'little' or 'big'")),
        }

        let signed_value = if signed {
            if bit_width > 0 && ((acc >> (bit_width - 1)) & 1) == 1 {
                (acc as i128) - (1i128 << bit_width)
            } else {
                acc as i128
            }
        } else {
            acc as i128
        };

        let value = i64::try_from(signed_value)
            .map_err(|_| runtime_err("int.from_bytes result out of i64 range"))?;
        Ok(Value::Int(value))
    }

    fn builtin_bool(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Ok(Value::Bool(false));
        };
        Ok(Value::Bool(value.is_truthy()))
    }

    fn builtin_sorted(&mut self, args: &[Value]) -> Result<Value> {
        let Some(iterable) = args.first() else {
            return Err(runtime_err("sorted() expects iterable"));
        };
        let mut values = self.iterable_to_values(iterable)?;
        values.sort_by(|left, right| match (left, right) {
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Str(a), Value::Str(b)) => a.cmp(b),
            _ => left.repr().cmp(&right.repr()),
        });
        Ok(Value::List(Rc::new(RefCell::new(values))))
    }

    fn builtin_enumerate(&mut self, args: &[Value]) -> Result<Value> {
        let Some(iterable) = args.first() else {
            return Err(runtime_err("enumerate() expects iterable"));
        };
        let start = args.get(1).map(expect_int).transpose()?.unwrap_or(0);
        let values = self.iterable_to_values(iterable)?;
        let mut out = Vec::with_capacity(values.len());
        for (index, value) in values.into_iter().enumerate() {
            out.push(Value::List(Rc::new(RefCell::new(vec![
                Value::Int(start + index as i64),
                value,
            ]))));
        }
        Ok(Value::List(Rc::new(RefCell::new(out))))
    }

    fn builtin_isinstance(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Err(runtime_err("isinstance() expects value"));
        };
        let Some(klass) = args.get(1) else {
            return Err(runtime_err("isinstance() expects type"));
        };
        Ok(Value::Bool(self.value_is_instance_of(value, klass)))
    }

    fn builtin_getattr(&mut self, args: &[Value]) -> Result<Value> {
        let Some(object) = args.first() else {
            return Err(runtime_err("getattr() expects object"));
        };
        let Some(name) = args.get(1) else {
            return Err(runtime_err("getattr() expects attribute name"));
        };
        let name = expect_str(name)?;
        match self.get_attr(object, &name) {
            Ok(value) => Ok(value),
            Err(err) => {
                if let Some(default) = args.get(2) {
                    Ok(default.clone())
                } else {
                    Err(err)
                }
            }
        }
    }

    fn builtin_hasattr(&mut self, args: &[Value]) -> Result<Value> {
        let Some(object) = args.first() else {
            return Err(runtime_err("hasattr() expects object"));
        };
        let Some(name) = args.get(1) else {
            return Err(runtime_err("hasattr() expects attribute name"));
        };
        let name = expect_str(name)?;
        Ok(Value::Bool(self.get_attr(object, &name).is_ok()))
    }

    fn builtin_any(&mut self, args: &[Value]) -> Result<Value> {
        let Some(iterable) = args.first() else {
            return Err(runtime_err("any() expects iterable"));
        };
        let values = self.iterable_to_values(iterable)?;
        Ok(Value::Bool(values.iter().any(Value::is_truthy)))
    }

    fn builtin_all(&mut self, args: &[Value]) -> Result<Value> {
        let Some(iterable) = args.first() else {
            return Err(runtime_err("all() expects iterable"));
        };
        let values = self.iterable_to_values(iterable)?;
        Ok(Value::Bool(values.iter().all(Value::is_truthy)))
    }

    fn builtin_zip(&mut self, args: &[Value]) -> Result<Value> {
        if args.is_empty() {
            return Ok(Value::List(Rc::new(RefCell::new(Vec::new()))));
        }
        let mut columns = Vec::new();
        for arg in args {
            columns.push(self.iterable_to_values(arg)?);
        }
        let min_len = columns.iter().map(Vec::len).min().unwrap_or(0);
        let mut out = Vec::with_capacity(min_len);
        for idx in 0..min_len {
            let mut row = Vec::with_capacity(columns.len());
            for col in &columns {
                row.push(col[idx].clone());
            }
            out.push(Value::List(Rc::new(RefCell::new(row))));
        }
        Ok(Value::List(Rc::new(RefCell::new(out))))
    }

    fn builtin_reversed(&mut self, args: &[Value]) -> Result<Value> {
        let Some(iterable) = args.first() else {
            return Err(runtime_err("reversed() expects iterable"));
        };
        let mut values = self.iterable_to_values(iterable)?;
        values.reverse();
        Ok(Value::List(Rc::new(RefCell::new(values))))
    }

    fn builtin_next(&mut self, args: &[Value]) -> Result<Value> {
        let Some(iterable) = args.first() else {
            return Err(runtime_err("next() expects iterator"));
        };
        match iterable {
            Value::Generator(generator) => {
                if let Some(value) = generator.borrow_mut().next() {
                    Ok(value)
                } else if let Some(default) = args.get(1) {
                    Ok(default.clone())
                } else {
                    Err(runtime_err("StopIteration"))
                }
            }
            Value::List(values) => {
                if let Some(value) = values.borrow().first() {
                    Ok(value.clone())
                } else if let Some(default) = args.get(1) {
                    Ok(default.clone())
                } else {
                    Err(runtime_err("StopIteration"))
                }
            }
            _ => Err(runtime_err("next() expects generator/list in this runtime")),
        }
    }

    fn builtin_file_read(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::File(state) = receiver else {
            return Err(runtime_err("file.read receiver must be file"));
        };
        let mut state = state.borrow_mut();
        let mut text = String::new();
        state
            .file
            .read_to_string(&mut text)
            .map_err(|err| NanoPythonError::Io(format!("read failed: {err}")))?;
        Ok(Value::Str(text))
    }

    fn builtin_file_write(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::File(state) = receiver else {
            return Err(runtime_err("file.write receiver must be file"));
        };
        let Some(text) = args.first() else {
            return Err(runtime_err("file.write expects content"));
        };
        let text = expect_str(text)?;
        let mut state = state.borrow_mut();
        state
            .file
            .write_all(text.as_bytes())
            .map_err(|err| NanoPythonError::Io(format!("write failed: {err}")))?;
        Ok(Value::Int(text.len() as i64))
    }

    fn builtin_file_close(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::File(state) = receiver else {
            return Err(runtime_err("file.close receiver must be file"));
        };
        state.borrow_mut().closed = true;
        Ok(Value::None)
    }

    fn builtin_file_enter(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        Ok(receiver.clone())
    }

    fn builtin_file_exit(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::File(state) = receiver else {
            return Err(runtime_err("file.__exit__ receiver must be file"));
        };
        state.borrow_mut().closed = true;
        Ok(Value::Bool(false))
    }

    fn builtin_path_resolve(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Path(path) = receiver else {
            return Err(runtime_err("path.resolve receiver must be path"));
        };
        let resolved = if let Ok(canon) = fs::canonicalize(path) {
            canon
        } else if path.is_absolute() {
            path.clone()
        } else {
            env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };
        Ok(Value::Path(resolved))
    }

    fn builtin_path_exists(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Path(path) = receiver else {
            return Err(runtime_err("path.exists receiver must be path"));
        };
        Ok(Value::Bool(path.exists()))
    }

    fn builtin_path_is_file(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Path(path) = receiver else {
            return Err(runtime_err("path.is_file receiver must be path"));
        };
        Ok(Value::Bool(path.is_file()))
    }

    fn builtin_path_is_dir(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Path(path) = receiver else {
            return Err(runtime_err("path.is_dir receiver must be path"));
        };
        Ok(Value::Bool(path.is_dir()))
    }

    fn builtin_path_read_text(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Path(path) = receiver else {
            return Err(runtime_err("path.read_text receiver must be path"));
        };
        match fs::read_to_string(path) {
            Ok(content) => Ok(Value::Str(content)),
            Err(_) => {
                let bytes = fs::read(path)
                    .map_err(|err| NanoPythonError::Io(format!("read_text failed: {err}")))?;
                Ok(Value::Str(String::from_utf8_lossy(&bytes).into_owned()))
            }
        }
    }

    fn builtin_path_write_text(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Path(path) = receiver else {
            return Err(runtime_err("path.write_text receiver must be path"));
        };
        let Some(value) = args.first() else {
            return Err(runtime_err("path.write_text expects content"));
        };
        let text = expect_str(value)?;
        fs::write(path, text.as_bytes())
            .map_err(|err| NanoPythonError::Io(format!("write_text failed: {err}")))?;
        Ok(Value::Int(text.len() as i64))
    }

    fn builtin_path_open(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Path(path) = receiver else {
            return Err(runtime_err("path.open receiver must be path"));
        };
        let mut call_args = vec![Value::Str(path.to_string_lossy().into_owned())];
        if let Some(mode) = args.first() {
            call_args.push(mode.clone());
        }
        self.builtin_open(&call_args)
    }

    fn builtin_path_relative_to(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Path(path) = receiver else {
            return Err(runtime_err("path.relative_to receiver must be path"));
        };
        let Some(base) = args.first() else {
            return Err(runtime_err("path.relative_to expects base path"));
        };
        let base = expect_path(base)?;
        let relative = path
            .strip_prefix(base)
            .map_err(|_| runtime_err("path is not under base path"))?;
        Ok(Value::Path(relative.to_path_buf()))
    }

    fn builtin_path_mkdir(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Path(path) = receiver else {
            return Err(runtime_err("path.mkdir receiver must be path"));
        };
        let parents = args.first().map(expect_bool).transpose()?.unwrap_or(false);
        let exist_ok = args.get(1).map(expect_bool).transpose()?.unwrap_or(false);
        let result = if parents {
            fs::create_dir_all(path)
        } else {
            fs::create_dir(path)
        };
        if let Err(err) = result {
            if exist_ok && err.kind() == std::io::ErrorKind::AlreadyExists {
                return Ok(Value::None);
            }
            return Err(NanoPythonError::Io(format!("mkdir failed: {err}")));
        }
        Ok(Value::None)
    }

    fn builtin_list_append(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::List(values) = receiver else {
            return Err(runtime_err("append() receiver must be list"));
        };
        let Some(value) = args.first() else {
            return Err(runtime_err("append() expects a value"));
        };
        values.borrow_mut().push(value.clone());
        Ok(Value::None)
    }

    fn builtin_list_extend(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::List(values) = receiver else {
            return Err(runtime_err("extend() receiver must be list"));
        };
        let Some(iterable) = args.first() else {
            return Err(runtime_err("extend() expects iterable"));
        };
        let expanded = self.iterable_to_values(iterable)?;
        values.borrow_mut().extend(expanded);
        Ok(Value::None)
    }

    fn builtin_list_pop(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::List(values) = receiver else {
            return Err(runtime_err("pop() receiver must be list"));
        };
        let mut values = values.borrow_mut();
        if values.is_empty() {
            return Err(runtime_err("pop from empty list"));
        }
        let index = if let Some(index) = args.first() {
            let index = expect_int(index)?;
            if index < 0 {
                (values.len() as i64 + index) as usize
            } else {
                index as usize
            }
        } else {
            values.len() - 1
        };
        if index >= values.len() {
            return Err(runtime_err("pop index out of range"));
        }
        Ok(values.remove(index))
    }

    fn builtin_list_sort(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::List(values) = receiver else {
            return Err(runtime_err("sort() receiver must be list"));
        };
        values.borrow_mut().sort_by(|left, right| match (left, right) {
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Str(a), Value::Str(b)) => a.cmp(b),
            _ => left.repr().cmp(&right.repr()),
        });
        Ok(Value::None)
    }

    fn builtin_dict_get(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Dict(values) = receiver else {
            return Err(runtime_err("dict.get receiver must be dict"));
        };
        let Some(key) = args.first() else {
            return Err(runtime_err("dict.get expects key"));
        };
        let key = key_of(key)?;
        let default = args.get(1).cloned().unwrap_or(Value::None);
        Ok(values.borrow().get(&key).cloned().unwrap_or(default))
    }

    fn builtin_dict_keys(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Dict(values) = receiver else {
            return Err(runtime_err("dict.keys receiver must be dict"));
        };
        let keys = values
            .borrow()
            .keys()
            .map(ValueKey::to_value)
            .collect::<Vec<_>>();
        Ok(Value::List(Rc::new(RefCell::new(keys))))
    }

    fn builtin_dict_values(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Dict(values) = receiver else {
            return Err(runtime_err("dict.values receiver must be dict"));
        };
        let values = values.borrow().values().cloned().collect::<Vec<_>>();
        Ok(Value::List(Rc::new(RefCell::new(values))))
    }

    fn builtin_dict_items(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Dict(values) = receiver else {
            return Err(runtime_err("dict.items receiver must be dict"));
        };
        let items = values
            .borrow()
            .iter()
            .map(|(key, value)| {
                Value::List(Rc::new(RefCell::new(vec![
                    key.to_value(),
                    value.clone(),
                ])))
            })
            .collect::<Vec<_>>();
        Ok(Value::List(Rc::new(RefCell::new(items))))
    }

    fn builtin_dict_setdefault(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Dict(values) = receiver else {
            return Err(runtime_err("dict.setdefault receiver must be dict"));
        };
        let Some(key) = args.first() else {
            return Err(runtime_err("dict.setdefault expects key"));
        };
        let key = key_of(key)?;
        if let Some(value) = values.borrow().get(&key) {
            return Ok(value.clone());
        }
        let default = args.get(1).cloned().unwrap_or(Value::None);
        values.borrow_mut().insert(key, default.clone());
        Ok(default)
    }

    fn builtin_set_add(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Set(values) = receiver else {
            return Err(runtime_err("set.add receiver must be set"));
        };
        let Some(value) = args.first() else {
            return Err(runtime_err("set.add expects value"));
        };
        let mut values = values.borrow_mut();
        if !contains_value(&values, value) {
            values.push(value.clone());
        }
        Ok(Value::None)
    }

    fn builtin_set_remove(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Set(values) = receiver else {
            return Err(runtime_err("set.remove receiver must be set"));
        };
        let Some(value) = args.first() else {
            return Err(runtime_err("set.remove expects value"));
        };
        let mut values = values.borrow_mut();
        let Some(index) = values.iter().position(|item| value_eq(item, value)) else {
            return Err(runtime_err("KeyError"));
        };
        values.remove(index);
        Ok(Value::None)
    }

    fn builtin_set_discard(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let Value::Set(values) = receiver else {
            return Err(runtime_err("set.discard receiver must be set"));
        };
        let Some(value) = args.first() else {
            return Err(runtime_err("set.discard expects value"));
        };
        let mut values = values.borrow_mut();
        if let Some(index) = values.iter().position(|item| value_eq(item, value)) {
            values.remove(index);
        }
        Ok(Value::None)
    }

    fn builtin_set_copy(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let Value::Set(values) = receiver else {
            return Err(runtime_err("set.copy receiver must be set"));
        };
        Ok(Value::Set(Rc::new(RefCell::new(values.borrow().clone()))))
    }

    fn builtin_dataclass(&mut self, args: &[Value]) -> Result<Value> {
        // Support both @dataclass and @dataclass(...)
        let Some(value) = args.first() else {
            return Ok(Value::BuiltinFunction(Self::builtin_dataclass_apply));
        };
        match value {
            Value::Class(class) => self.make_dataclass(class.clone()),
            _ => Ok(Value::BuiltinFunction(Self::builtin_dataclass_apply)),
        }
    }

    fn builtin_dataclass_apply(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Err(runtime_err("dataclass decorator expects class"));
        };
        let Value::Class(class) = value else {
            return Err(runtime_err("dataclass decorator expects class"));
        };
        self.make_dataclass(class.clone())
    }

    fn make_dataclass(&mut self, class: Rc<ClassDef>) -> Result<Value> {
        let mut attrs = class.attrs.clone();
        let mut fields = Vec::new();
        if let Some(Value::List(declared)) = attrs.get("__decl_order__") {
            for item in declared.borrow().iter() {
                let Value::Str(name) = item else {
                    continue;
                };
                if name.starts_with("__") {
                    continue;
                }
                let Some(value) = attrs.get(name) else {
                    continue;
                };
                if matches!(
                    value,
                    Value::Function(_)
                        | Value::Lambda(_)
                        | Value::ClassMethod(_)
                        | Value::Property(_)
                        | Value::BuiltinFunction(_)
                        | Value::BuiltinMethod { .. }
                        | Value::BoundMethod { .. }
                        | Value::Class(_)
                        | Value::Module(_)
                ) {
                    continue;
                }
                fields.push(Value::Str(name.clone()));
            }
        }
        attrs.insert(
            "__dataclass_fields__".to_owned(),
            Value::List(Rc::new(RefCell::new(fields))),
        );
        Ok(Value::Class(Rc::new(ClassDef {
            name: class.name.clone(),
            attrs,
            bases: class.bases.clone(),
        })))
    }

    fn builtin_identity_decorator(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Err(runtime_err("decorator expects callable"));
        };
        Ok(value.clone())
    }

    fn builtin_classmethod(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Err(runtime_err("classmethod expects function"));
        };
        Ok(Value::ClassMethod(Box::new(value.clone())))
    }

    fn builtin_property(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Err(runtime_err("property expects getter"));
        };
        Ok(Value::Property(Box::new(value.clone())))
    }

    fn builtin_field(&mut self, args: &[Value]) -> Result<Value> {
        if let Some(default) = args.first() {
            if let Value::BuiltinFunction(func) = default {
                return func(self, &[]);
            }
            return Ok(default.clone());
        }
        Ok(Value::None)
    }

    fn builtin_enum_auto(&mut self, _args: &[Value]) -> Result<Value> {
        Ok(Value::EnumAuto)
    }

    fn builtin_copy_deepcopy(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Err(runtime_err("copy.deepcopy expects value"));
        };
        Ok(deep_clone_value(value))
    }

    fn builtin_os_remove(&mut self, args: &[Value]) -> Result<Value> {
        let Some(path) = args.first() else {
            return Err(runtime_err("os.remove expects path"));
        };
        let path = expect_str(path)?;
        fs::remove_file(&path)
            .map_err(|err| NanoPythonError::Io(format!("remove_file({path}) failed: {err}")))?;
        Ok(Value::None)
    }

    fn builtin_gc_collect(&mut self, _args: &[Value]) -> Result<Value> {
        Ok(Value::Int(0))
    }

    fn builtin_gc_get_threshold(&mut self, _args: &[Value]) -> Result<Value> {
        Ok(Value::List(Rc::new(RefCell::new(vec![
            Value::Int(700),
            Value::Int(10),
            Value::Int(10),
        ]))))
    }

    fn builtin_warnings_warn(&mut self, _args: &[Value]) -> Result<Value> {
        Ok(Value::None)
    }

    fn builtin_sys_exit(&mut self, _args: &[Value]) -> Result<Value> {
        let code = match _args.first() {
            Some(value) => expect_int(value).unwrap_or(0),
            None => 0,
        };
        Err(runtime_err(&format!("SystemExit:{code}")))
    }

    fn builtin_keyword_iskeyword(&mut self, args: &[Value]) -> Result<Value> {
        let Some(value) = args.first() else {
            return Ok(Value::Bool(false));
        };
        let text = expect_str(value)?;
        Ok(Value::Bool(is_python_keyword(&text)))
    }

    fn builtin_argparse_argument_parser(&mut self, args: &[Value]) -> Result<Value> {
        let prog = args
            .iter()
            .find_map(|value| match value {
                Value::Str(text) if !text.is_empty() => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "prog".to_owned());
        let description = args
            .iter()
            .rev()
            .find_map(|value| match value {
                Value::Str(text) if text.contains(' ') => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();
        Ok(Value::ArgParser(Rc::new(RefCell::new(ArgParserState {
            prog,
            description,
            options: Vec::new(),
        }))))
    }

    fn builtin_argparse_add_argument(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let state = match receiver {
            Value::ArgParser(state) | Value::ArgGroup(state) => state,
            _ => return Err(runtime_err("add_argument receiver must be argparse parser/group")),
        };
        let option = build_arg_option(args)?;
        state.borrow_mut().options.push(option);
        Ok(Value::None)
    }

    fn builtin_argparse_add_group(&mut self, receiver: &Value, _args: &[Value]) -> Result<Value> {
        let state = match receiver {
            Value::ArgParser(state) => state.clone(),
            _ => return Err(runtime_err("add_mutually_exclusive_group receiver must be parser")),
        };
        Ok(Value::ArgGroup(state))
    }

    fn builtin_argparse_parse_args(&mut self, receiver: &Value, args: &[Value]) -> Result<Value> {
        let state = match receiver {
            Value::ArgParser(state) | Value::ArgGroup(state) => state.clone(),
            _ => return Err(runtime_err("parse_args receiver must be parser")),
        };
        let state = state.borrow();

        let input = if let Some(value) = args.first() {
            values_to_strings(value)?
        } else if self.argv.len() > 1 {
            self.argv[1..].to_vec()
        } else {
            Vec::new()
        };
        if input.iter().any(|arg| arg == "-h" || arg == "--help") {
            if state.description.is_empty() {
                println!("usage: {}", state.prog);
            } else {
                println!("usage: {}\n\n{}", state.prog, state.description);
            }
            return Err(runtime_err("SystemExit:0"));
        }

        let mut attrs = HashMap::new();
        let positional_options: Vec<&ArgOption> = state.options.iter().filter(|opt| opt.positional).collect();
        for opt in &state.options {
            let default = match opt.action {
                ArgAction::StoreTrue => Value::Bool(false),
                ArgAction::Append => Value::List(Rc::new(RefCell::new(Vec::new()))),
                ArgAction::Store if opt.nargs_star => Value::List(Rc::new(RefCell::new(Vec::new()))),
                _ => opt.default.clone(),
            };
            attrs.insert(opt.dest.clone(), default);
        }

        let mut positional_index = 0usize;
        let mut idx = 0usize;
        while idx < input.len() {
            let token = &input[idx];
            if token.starts_with('-') {
                let Some(option) = state
                    .options
                    .iter()
                    .find(|opt| !opt.positional && opt.flags.iter().any(|flag| flag == token))
                else {
                    idx += 1;
                    continue;
                };
                match option.action {
                    ArgAction::StoreTrue => {
                        attrs.insert(option.dest.clone(), Value::Bool(true));
                        idx += 1;
                    }
                    ArgAction::Append => {
                        if idx + 1 >= input.len() {
                            return Err(runtime_err("missing value for option"));
                        }
                        let value = convert_arg_value(&input[idx + 1], option.arg_type)?;
                        let entry = attrs
                            .entry(option.dest.clone())
                            .or_insert_with(|| Value::List(Rc::new(RefCell::new(Vec::new()))));
                        if let Value::List(values) = entry {
                            values.borrow_mut().push(value);
                        }
                        idx += 2;
                    }
                    ArgAction::Store => {
                        if idx + 1 >= input.len() {
                            return Err(runtime_err("missing value for option"));
                        }
                        let value = convert_arg_value(&input[idx + 1], option.arg_type)?;
                        attrs.insert(option.dest.clone(), value);
                        idx += 2;
                    }
                }
                continue;
            }

            if let Some(option) = positional_options.get(positional_index) {
                if option.nargs_star {
                    let mut values = Vec::new();
                    while idx < input.len() {
                        let lookahead = &input[idx];
                        if lookahead.starts_with('-')
                            && state.options.iter().any(|candidate| {
                                !candidate.positional
                                    && candidate.flags.iter().any(|flag| flag == lookahead)
                            })
                        {
                            break;
                        }
                        values.push(convert_arg_value(lookahead, option.arg_type)?);
                        idx += 1;
                    }
                    attrs.insert(
                        option.dest.clone(),
                        Value::List(Rc::new(RefCell::new(values))),
                    );
                    positional_index += 1;
                } else {
                    attrs.insert(option.dest.clone(), convert_arg_value(token, option.arg_type)?);
                    idx += 1;
                    positional_index += 1;
                }
            } else {
                idx += 1;
            }
        }

        Ok(make_namespace(attrs))
    }
}

fn is_blocked_module(module_name: &str) -> bool {
    let root = module_name.split('.').next().unwrap_or(module_name);
    matches!(
        root,
        "asyncio" | "threading" | "multiprocessing" | "pickle" | "socket"
    )
}

fn build_arg_option(args: &[Value]) -> Result<ArgOption> {
    if args.is_empty() {
        return Err(runtime_err("add_argument requires at least one name/flag"));
    }

    let mut flags = Vec::new();
    let mut index = 0usize;
    while let Some(Value::Str(flag)) = args.get(index) {
        if flag.starts_with('-') {
            flags.push(flag.clone());
            index += 1;
            continue;
        }
        break;
    }

    let positional = flags.is_empty();
    let mut action = ArgAction::Store;
    let mut nargs_star = false;
    let mut arg_type = ArgType::String;
    let mut default = Value::None;
    let mut dest_override = None;

    if positional {
        let Some(Value::Str(name)) = args.get(index) else {
            return Err(runtime_err("positional argument requires a destination name"));
        };
        dest_override = Some(name.clone());
        index += 1;
    }

    for value in &args[index..] {
        match value {
            Value::Str(text) => {
                if text == "store_true" {
                    action = ArgAction::StoreTrue;
                    continue;
                }
                if text == "append" {
                    action = ArgAction::Append;
                    continue;
                }
                if text == "*" {
                    nargs_star = true;
                    continue;
                }
                // In this minimal runtime keyword names are dropped, so `dest="name"` is
                // observed as a bare string after flags.
                if !positional
                    && flags.len() > 1
                    && dest_override.is_none()
                    && is_dest_like(text)
                {
                    dest_override = Some(text.clone());
                    continue;
                }
                if matches!(default, Value::None) && is_string_default_like(text) {
                    default = Value::Str(text.clone());
                }
            }
            Value::BuiltinFunction(func) => {
                if (*func as usize) == (SelfVm::builtin_path_ctor as usize) {
                    arg_type = ArgType::Path;
                } else if (*func as usize) == (SelfVm::builtin_int as usize) {
                    arg_type = ArgType::Int;
                } else {
                    arg_type = ArgType::String;
                }
            }
            Value::Path(_)
            | Value::Int(_)
            | Value::Bool(_)
            | Value::None
            | Value::List(_)
            | Value::Dict(_)
            | Value::Set(_) => {
                default = value.clone();
            }
            _ => {}
        }
    }

    let dest = if let Some(dest) = dest_override {
        dest
    } else if positional {
        return Err(runtime_err("positional argument requires a destination name"));
    } else {
        derive_dest_from_flags(&flags)?
    };

    Ok(ArgOption {
        flags,
        dest,
        positional,
        action,
        nargs_star,
        arg_type,
        default,
    })
}

fn is_dest_like(text: &str) -> bool {
    if text.is_empty() || text.contains(' ') || text.starts_with('-') {
        return false;
    }
    if text == "*" || text == "store_true" || text == "append" {
        return false;
    }
    text.chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        && text.chars().any(|ch| ch.is_ascii_lowercase())
}

fn is_string_default_like(text: &str) -> bool {
    if text.is_empty() || text.contains(' ') || text.starts_with('-') {
        return false;
    }
    if text == "*" || text == "store_true" || text == "append" {
        return false;
    }
    let uppercase_like = text
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_');
    !uppercase_like
}

fn derive_dest_from_flags(flags: &[String]) -> Result<String> {
    let Some(flag) = flags
        .iter()
        .find(|flag| flag.starts_with("--"))
        .or_else(|| flags.first())
    else {
        return Err(runtime_err("optional argument requires flag"));
    };

    let trimmed = flag.trim_start_matches('-').replace('-', "_");
    if trimmed.is_empty() {
        return Err(runtime_err("invalid argparse flag"));
    }
    Ok(trimmed)
}

fn values_to_strings(value: &Value) -> Result<Vec<String>> {
    match value {
        Value::List(values) => values
            .borrow()
            .iter()
            .map(expect_str)
            .collect::<Result<Vec<_>>>(),
        Value::Set(values) => values
            .borrow()
            .iter()
            .map(expect_str)
            .collect::<Result<Vec<_>>>(),
        Value::None => Ok(Vec::new()),
        other => Ok(vec![expect_str(other)?]),
    }
}

fn convert_arg_value(token: &str, arg_type: ArgType) -> Result<Value> {
    match arg_type {
        ArgType::String => Ok(Value::Str(token.to_owned())),
        ArgType::Path => Ok(Value::Path(PathBuf::from(token))),
        ArgType::Int => token
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| runtime_err("invalid integer argument")),
    }
}

fn make_namespace(attrs: HashMap<String, Value>) -> Value {
    Value::Instance(Rc::new(RefCell::new(Instance {
        class: Rc::new(ClassDef {
            name: "Namespace".to_owned(),
            attrs: HashMap::new(),
            bases: Vec::new(),
        }),
        attrs,
    })))
}

fn is_python_keyword(text: &str) -> bool {
    matches!(
        text,
        "False"
            | "None"
            | "True"
            | "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "class"
            | "continue"
            | "def"
            | "del"
            | "elif"
            | "else"
            | "except"
            | "finally"
            | "for"
            | "from"
            | "global"
            | "if"
            | "import"
            | "in"
            | "is"
            | "lambda"
            | "nonlocal"
            | "not"
            | "or"
            | "pass"
            | "raise"
            | "return"
            | "try"
            | "while"
            | "with"
            | "yield"
    )
}

fn deep_clone_value(value: &Value) -> Value {
    match value {
        Value::None => Value::None,
        Value::Bool(v) => Value::Bool(*v),
        Value::Int(v) => Value::Int(*v),
        Value::Str(v) => Value::Str(v.clone()),
        Value::Bytes(v) => Value::Bytes(v.clone()),
        Value::Path(v) => Value::Path(v.clone()),
        Value::EnumMember {
            enum_name,
            name,
            value,
        } => Value::EnumMember {
            enum_name: enum_name.clone(),
            name: name.clone(),
            value: *value,
        },
        Value::EnumAuto => Value::EnumAuto,
        Value::List(values) => Value::List(Rc::new(RefCell::new(
            values.borrow().iter().map(deep_clone_value).collect(),
        ))),
        Value::Set(values) => Value::Set(Rc::new(RefCell::new(
            values.borrow().iter().map(deep_clone_value).collect(),
        ))),
        Value::Dict(values) => {
            let mut out = BTreeMap::new();
            for (key, value) in values.borrow().iter() {
                out.insert(key.clone(), deep_clone_value(value));
            }
            Value::Dict(Rc::new(RefCell::new(out)))
        }
        Value::Instance(instance) => {
            let instance_ref = instance.borrow();
            let mut attrs = HashMap::new();
            for (key, value) in instance_ref.attrs.iter() {
                attrs.insert(key.clone(), deep_clone_value(value));
            }
            Value::Instance(Rc::new(RefCell::new(Instance {
                class: instance_ref.class.clone(),
                attrs,
            })))
        }
        Value::Generator(generator) => {
            let state = generator.borrow();
            Value::Generator(Rc::new(RefCell::new(GeneratorState {
                values: state.values.iter().map(deep_clone_value).collect(),
                index: state.index,
            })))
        }
        // These are immutable for interpreter behavior, so a shallow clone is acceptable.
        Value::Function(_)
        | Value::Lambda(_)
        | Value::ClassMethod(_)
        | Value::Property(_)
        | Value::BuiltinFunction(_)
        | Value::BuiltinMethod { .. }
        | Value::BoundMethod { .. }
        | Value::Class(_)
        | Value::Range { .. }
        | Value::Module(_)
        | Value::File(_)
        | Value::ArgParser(_)
        | Value::ArgGroup(_) => value.clone(),
    }
}

fn contains_value(values: &[Value], needle: &Value) -> bool {
    values.iter().any(|value| value_eq(value, needle))
}

fn value_eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::None, Value::None) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Bytes(a), Value::Bytes(b)) => a == b,
        (Value::Path(a), Value::Path(b)) => a == b,
        (
            Value::EnumMember {
                enum_name: a_enum,
                name: a_name,
                ..
            },
            Value::EnumMember {
                enum_name: b_enum,
                name: b_name,
                ..
            },
        ) => a_enum == b_enum && a_name == b_name,
        _ => false,
    }
}

fn error_kind_and_message(err: &NanoPythonError) -> (String, String) {
    match err {
        NanoPythonError::Io(message) => ("OSError".to_owned(), message.clone()),
        NanoPythonError::Parse(message) => ("SyntaxError".to_owned(), message.clone()),
        NanoPythonError::Runtime(message) => {
            if let Some((name, rest)) = message.split_once(':') {
                let name = name.trim();
                if !name.is_empty() {
                    return (name.to_owned(), rest.trim().to_owned());
                }
            }
            ("RuntimeError".to_owned(), message.clone())
        }
        NanoPythonError::PluginRegistration(message)
        | NanoPythonError::PluginValidation(message) => ("RuntimeError".to_owned(), message.clone()),
        NanoPythonError::PluginBootstrap { message, .. } => {
            ("RuntimeError".to_owned(), message.clone())
        }
    }
}

fn exception_from_value(value: &Value) -> (String, String) {
    match value {
        Value::Class(class) => (class.name.clone(), String::new()),
        Value::Instance(instance) => {
            let instance = instance.borrow();
            let message = instance
                .attrs
                .get("message")
                .map(Value::repr)
                .unwrap_or_default();
            (instance.class.name.clone(), message)
        }
        Value::Str(message) => ("RuntimeError".to_owned(), message.clone()),
        _ => ("RuntimeError".to_owned(), value.repr()),
    }
}

fn exception_matches(expected: &Value, actual_name: &str) -> bool {
    match expected {
        Value::Class(class) => class.name == actual_name,
        Value::List(values) => values
            .borrow()
            .iter()
            .any(|value| exception_matches(value, actual_name)),
        Value::Str(name) => name == actual_name,
        _ => false,
    }
}

fn normalize_slice_bounds(len: usize, start: Option<i64>, end: Option<i64>) -> (usize, usize) {
    let len_i64 = len as i64;
    let mut start = start.unwrap_or(0);
    let mut end = end.unwrap_or(len_i64);
    if start < 0 {
        start += len_i64;
    }
    if end < 0 {
        end += len_i64;
    }
    let start = start.clamp(0, len_i64) as usize;
    let end = end.clamp(0, len_i64) as usize;
    (start.min(len), end.min(len))
}

fn slice_indices(len: usize, start: Option<i64>, end: Option<i64>, step: i64) -> Vec<usize> {
    let len_i64 = len as i64;
    let mut out = Vec::new();
    if len == 0 {
        return out;
    }

    if step > 0 {
        let mut start = start.unwrap_or(0);
        let mut end = end.unwrap_or(len_i64);
        if start < 0 {
            start += len_i64;
        }
        if end < 0 {
            end += len_i64;
        }
        let mut i = start.clamp(0, len_i64);
        let end = end.clamp(0, len_i64);
        while i < end {
            out.push(i as usize);
            i += step;
        }
        return out;
    }

    let mut start = start.unwrap_or(len_i64 - 1);
    let mut end = end.unwrap_or(-1);
    if start < 0 {
        start += len_i64;
    }
    if end < 0 {
        end += len_i64;
    }
    let mut i = start.clamp(-1, len_i64 - 1);
    let end = end.clamp(-1, len_i64 - 1);
    while i > end {
        out.push(i as usize);
        i += step;
    }
    out
}

fn split_fstring_field(field: &str) -> (&str, bool) {
    let mut in_single = false;
    let mut in_double = false;
    let mut depth = 0i32;
    let chars: Vec<char> = field.chars().collect();
    let mut conv_idx = None;
    let mut fmt_idx = None;
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '\\' => i += 1,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' | '[' | '{' if !in_single && !in_double => depth += 1,
            ')' | ']' | '}' if !in_single && !in_double => depth -= 1,
            '!' if !in_single && !in_double && depth == 0 && conv_idx.is_none() => {
                conv_idx = Some(i);
            }
            ':' if !in_single && !in_double && depth == 0 && fmt_idx.is_none() => {
                fmt_idx = Some(i);
            }
            _ => {}
        }
        i += 1;
    }

    let mut end = field.len();
    if let Some(idx) = fmt_idx {
        end = end.min(idx);
    }
    if let Some(idx) = conv_idx {
        end = end.min(idx);
    }
    let repr_mode = conv_idx
        .and_then(|idx| chars.get(idx + 1))
        .is_some_and(|ch| *ch == 'r');
    (field[..end].trim(), repr_mode)
}

fn repr_bytes(bytes: &[u8]) -> String {
    let mut out = String::from("b'");
    for byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\'' => out.push_str("\\'"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(*byte as char),
            _ => out.push_str(&format!("\\x{:02x}", byte)),
        }
    }
    out.push('\'');
    out
}

fn py_repr(value: &Value) -> String {
    match value {
        Value::Str(text) => {
            let mut out = String::from("'");
            for ch in text.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    '\'' => out.push_str("\\'"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    ch if ch.is_control() => out.push_str(&format!("\\x{:02x}", ch as u32)),
                    ch => out.push(ch),
                }
            }
            out.push('\'');
            out
        }
        Value::Bytes(bytes) => repr_bytes(bytes),
        Value::List(values) => {
            let parts: Vec<String> = values.borrow().iter().map(py_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Dict(values) => {
            let parts: Vec<String> = values
                .borrow()
                .iter()
                .map(|(key, value)| format!("{}: {}", py_repr(&key.to_value()), py_repr(value)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        Value::Set(values) => {
            let parts: Vec<String> = values.borrow().iter().map(py_repr).collect();
            format!("{{{}}}", parts.join(", "))
        }
        _ => value.repr(),
    }
}

fn value_is(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::None, Value::None) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Bytes(a), Value::Bytes(b)) => a == b,
        (
            Value::EnumMember {
                enum_name: a_enum,
                name: a_name,
                ..
            },
            Value::EnumMember {
                enum_name: b_enum,
                name: b_name,
                ..
            },
        ) => a_enum == b_enum && a_name == b_name,
        (Value::List(a), Value::List(b)) => Rc::ptr_eq(a, b),
        (Value::Dict(a), Value::Dict(b)) => Rc::ptr_eq(a, b),
        (Value::Set(a), Value::Set(b)) => Rc::ptr_eq(a, b),
        (Value::Function(a), Value::Function(b)) => Rc::ptr_eq(a, b),
        (Value::Lambda(a), Value::Lambda(b)) => Rc::ptr_eq(a, b),
        (Value::Class(a), Value::Class(b)) => Rc::ptr_eq(a, b),
        (Value::Instance(a), Value::Instance(b)) => Rc::ptr_eq(a, b),
        (Value::Generator(a), Value::Generator(b)) => Rc::ptr_eq(a, b),
        (Value::Module(a), Value::Module(b)) => Rc::ptr_eq(a, b),
        (Value::File(a), Value::File(b)) => Rc::ptr_eq(a, b),
        (Value::Path(a), Value::Path(b)) => a == b,
        (Value::ArgParser(a), Value::ArgParser(b)) => Rc::ptr_eq(a, b),
        (Value::ArgGroup(a), Value::ArgGroup(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

fn value_in(left: &Value, right: &Value) -> Result<bool> {
    match right {
        Value::Str(haystack) => {
            let needle = expect_str(left)?;
            Ok(haystack.contains(&needle))
        }
        Value::Bytes(bytes) => {
            let needle = expect_int(left)?;
            if !(0..=255).contains(&needle) {
                return Ok(false);
            }
            Ok(bytes.contains(&(needle as u8)))
        }
        Value::List(values) => Ok(values.borrow().iter().any(|v| value_eq(v, left))),
        Value::Set(values) => Ok(values.borrow().iter().any(|v| value_eq(v, left))),
        Value::Dict(values) => {
            let key = key_of(left)?;
            Ok(values.borrow().contains_key(&key))
        }
        Value::Range { start, stop, step } => {
            let value = expect_int(left)?;
            if *step == 0 {
                return Err(runtime_err("range step cannot be zero"));
            }
            if *step > 0 {
                if value < *start || value >= *stop {
                    return Ok(false);
                }
            } else if value > *start || value <= *stop {
                return Ok(false);
            }
            Ok((value - *start) % *step == 0)
        }
        _ => Err(runtime_err("right operand is not a container")),
    }
}

fn expect_int(value: &Value) -> Result<i64> {
    match value {
        Value::Int(v) => Ok(*v),
        Value::Bool(v) => Ok(if *v { 1 } else { 0 }),
        Value::EnumMember { value, .. } => Ok(*value),
        _ => Err(runtime_err("expected integer")),
    }
}

fn expect_str(value: &Value) -> Result<String> {
    match value {
        Value::Str(v) => Ok(v.clone()),
        Value::Path(v) => Ok(v.to_string_lossy().into_owned()),
        _ => Err(runtime_err("expected string")),
    }
}

fn expect_bool(value: &Value) -> Result<bool> {
    match value {
        Value::Bool(v) => Ok(*v),
        _ => Err(runtime_err("expected bool")),
    }
}

fn expect_path(value: &Value) -> Result<PathBuf> {
    match value {
        Value::Path(path) => Ok(path.clone()),
        Value::Str(path) => Ok(PathBuf::from(path)),
        _ => Err(runtime_err("expected path-like value")),
    }
}

fn key_of(value: &Value) -> Result<ValueKey> {
    ValueKey::from_value(value)
}

fn runtime_err(message: &str) -> NanoPythonError {
    NanoPythonError::Runtime(message.to_owned())
}

fn add_stmt_context(err: NanoPythonError, stmt: &Stmt) -> NanoPythonError {
    match err {
        NanoPythonError::Runtime(message) => {
            NanoPythonError::Runtime(format!("{message} [stmt: {stmt:?}]"))
        }
        NanoPythonError::Io(message) => NanoPythonError::Io(format!("{message} [stmt: {stmt:?}]")),
        NanoPythonError::Parse(message) => {
            NanoPythonError::Parse(format!("{message} [stmt: {stmt:?}]"))
        }
        NanoPythonError::PluginRegistration(message) => NanoPythonError::PluginRegistration(message),
        NanoPythonError::PluginValidation(message) => NanoPythonError::PluginValidation(message),
        NanoPythonError::PluginBootstrap { plugin, message } => {
            NanoPythonError::PluginBootstrap { plugin, message }
        }
    }
}

fn parse_system_exit_code(message: &str) -> ExitCode {
    let code = message
        .split_once(':')
        .and_then(|(_, code)| code.trim().parse::<u8>().ok())
        .unwrap_or(0);
    ExitCode::from(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> Result<()> {
        let mut vm = SelfVm::default();
        vm.execute_source(src, "<test>")
    }

    #[test]
    fn executes_control_flow_and_range() {
        let src = r#"
acc = 0
for i in range(0, 6):
    if i % 2 == 0:
        acc = acc + i
assert_value = acc
"#;
        let mut vm = SelfVm::default();
        let parsed = parse_source(src).expect("parse");
        let env = vm.new_global_env();
        let mut state = ExecState::default();
        vm.exec_block(&parsed.body, &env, &mut state).expect("exec");
        let result = env_get(&env, "assert_value").expect("missing var");
        assert!(matches!(result, Value::Int(6)));
    }

    #[test]
    fn executes_with_and_file_io() {
        let src = r#"
with open("selfvm_tmp.txt", "w") as f:
    f.write("42")
with open("selfvm_tmp.txt", "r") as f:
    result = f.read()
import os
os.remove("selfvm_tmp.txt")
"#;
        let mut vm = SelfVm::default();
        let parsed = parse_source(src).expect("parse");
        let env = vm.new_global_env();
        let mut state = ExecState::default();
        vm.exec_block(&parsed.body, &env, &mut state).expect("exec");
        let result = env_get(&env, "result").expect("result missing");
        assert!(matches!(result, Value::Str(ref s) if s == "42"));
    }

    #[test]
    fn executes_generator_yield() {
        let src = r#"
def gen(n):
    i = 0
    while i < n:
        yield i
        i = i + 1
values = list(gen(3))
"#;
        let mut vm = SelfVm::default();
        let parsed = parse_source(src).expect("parse");
        let env = vm.new_global_env();
        let mut state = ExecState::default();
        vm.exec_block(&parsed.body, &env, &mut state).expect("exec");
        let values = env_get(&env, "values").expect("missing values");
        match values {
            Value::List(values) => {
                let values = values.borrow();
                assert_eq!(values.len(), 3);
                assert!(matches!(values[0], Value::Int(0)));
                assert!(matches!(values[1], Value::Int(1)));
                assert!(matches!(values[2], Value::Int(2)));
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn class_attributes_are_visible() {
        let src = r#"
class Item:
    value = 7
x = Item.value
"#;
        let mut vm = SelfVm::default();
        let parsed = parse_source(src).expect("parse");
        let env = vm.new_global_env();
        let mut state = ExecState::default();
        vm.exec_block(&parsed.body, &env, &mut state).expect("exec");
        let x = env_get(&env, "x").expect("missing x");
        assert!(matches!(x, Value::Int(7)));
    }

    #[test]
    fn can_parse_and_run_simple_module() {
        run("x = 1\ny = x + 2\n").expect("run failed");
    }

    #[test]
    fn supports_bytes_and_int_from_bytes() {
        let src = r#"
data = "ABCD".encode("utf-8")
value = int.from_bytes(data[0:4], "little")
third = data[2]
tail = data[1:]
tail_len = len(tail)
"#;
        let mut vm = SelfVm::default();
        let parsed = parse_source(src).expect("parse");
        let env = vm.new_global_env();
        let mut state = ExecState::default();
        vm.exec_block(&parsed.body, &env, &mut state).expect("exec");
        let value = env_get(&env, "value").expect("missing value");
        assert!(matches!(value, Value::Int(1145258561)));
        let third = env_get(&env, "third").expect("missing third");
        assert!(matches!(third, Value::Int(67)));
        let tail_len = env_get(&env, "tail_len").expect("missing tail_len");
        assert!(matches!(tail_len, Value::Int(3)));
    }

    #[test]
    fn dict_keeps_integer_keys() {
        let src = r#"
d = {}
d[1] = "one"
_ = d.setdefault(2, "two")
first = d[1]
second = d[2]
keys = sorted(d.keys())
"#;
        let mut vm = SelfVm::default();
        let parsed = parse_source(src).expect("parse");
        let env = vm.new_global_env();
        let mut state = ExecState::default();
        vm.exec_block(&parsed.body, &env, &mut state).expect("exec");
        let first = env_get(&env, "first").expect("missing first");
        assert!(matches!(first, Value::Str(ref s) if s == "one"));
        let second = env_get(&env, "second").expect("missing second");
        assert!(matches!(second, Value::Str(ref s) if s == "two"));
        let keys = env_get(&env, "keys").expect("missing keys");
        match keys {
            Value::List(values) => {
                let values = values.borrow();
                assert!(matches!(values[0], Value::Int(1)));
                assert!(matches!(values[1], Value::Int(2)));
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn repr_and_fstring_expression_braces_work() {
        let src = r#"
text = "a'b"
quoted = repr(text)
name = "Foo"
def identity(v):
    return v
formatted = f"{identity(name + ' { ')}"
"#;
        let mut vm = SelfVm::default();
        let parsed = parse_source(src).expect("parse");
        let env = vm.new_global_env();
        let mut state = ExecState::default();
        vm.exec_block(&parsed.body, &env, &mut state).expect("exec");
        let quoted = env_get(&env, "quoted").expect("missing quoted");
        assert!(matches!(quoted, Value::Str(ref s) if s == "'a\\'b'"));
        let formatted = env_get(&env, "formatted").expect("missing formatted");
        assert!(matches!(formatted, Value::Str(ref s) if s == "Foo { "));
    }
}
