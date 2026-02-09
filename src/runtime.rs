use crate::ast::{BinaryOp, Expr, Param, ParamKind, Program, Stmt, UnaryOp};
use crate::lexer;
use crate::parser;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub type RtResult<T> = Result<T, String>;

#[derive(Clone)]
pub struct Env(Rc<RefCell<Scope>>);

#[derive(Clone)]
struct Scope {
    values: HashMap<String, Value>,
    parent: Option<Env>,
}

impl Env {
    fn new(parent: Option<Env>) -> Self {
        Self(Rc::new(RefCell::new(Scope {
            values: HashMap::new(),
            parent,
        })))
    }

    fn get(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.0.borrow().values.get(name) {
            return Some(v.clone());
        }
        self.0.borrow().parent.as_ref().and_then(|p| p.get(name))
    }

    fn set_local(&self, name: impl Into<String>, value: Value) {
        self.0.borrow_mut().values.insert(name.into(), value);
    }

    fn values_snapshot(&self) -> HashMap<String, Value> {
        self.0.borrow().values.clone()
    }
}

#[derive(Clone)]
pub enum Value {
    None,
    Bool(bool),
    Int(i64),
    Str(String),
    Bytes(Rc<RefCell<Vec<u8>>>),
    List(Rc<RefCell<Vec<Value>>>),
    Dict(Rc<RefCell<BTreeMap<Key, Value>>>),
    Set(Rc<RefCell<BTreeSet<Key>>>),
    Function(Rc<Function>),
    ClassMethod(Rc<Function>),
    StaticMethod(Box<Value>),
    Property(Rc<Function>),
    Builtin(Builtin),
    Class(Rc<Class>),
    BoundMethod(Rc<BoundMethod>),
    BoundClassMethod(Rc<BoundClassMethod>),
    Instance(Rc<RefCell<Instance>>),
    Module(Rc<RefCell<Module>>),
    File(Rc<RefCell<FileHandle>>),
    Path(PathBuf),
    Namespace(Rc<RefCell<HashMap<String, Value>>>),
    ArgParser(Rc<RefCell<ArgParser>>),
    Super(Rc<SuperObject>),
    FieldSpec(Rc<FieldSpecValue>),
    AutoEnum,
    TypingAlias(String),
    Range(RangeValue),
    Generator(Rc<RefCell<Generator>>),
}

#[derive(Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub env: Env,
    pub is_generator: bool,
}

#[derive(Clone)]
pub enum Builtin {
    Native(fn(&mut Interpreter, Vec<Value>, HashMap<String, Value>) -> RtResult<Value>),
    BoundFile {
        file: Rc<RefCell<FileHandle>>,
        method: FileMethod,
    },
    BoundList {
        list: Rc<RefCell<Vec<Value>>>,
        method: ListMethod,
    },
    BoundDict {
        dict: Rc<RefCell<BTreeMap<Key, Value>>>,
        method: DictMethod,
    },
    BoundSet {
        set: Rc<RefCell<BTreeSet<Key>>>,
        method: SetMethod,
    },
    BoundString {
        value: String,
        method: StringMethod,
    },
    BoundBytes {
        value: Rc<RefCell<Vec<u8>>>,
        method: BytesMethod,
    },
    BoundPath {
        value: PathBuf,
        method: PathMethod,
    },
    BoundNamespace {
        value: Rc<RefCell<HashMap<String, Value>>>,
        method: NamespaceMethod,
    },
    BoundArgParser {
        value: Rc<RefCell<ArgParser>>,
        method: ArgParserMethod,
    },
}

#[derive(Clone)]
pub struct Class {
    pub name: String,
    pub attrs: RefCell<HashMap<String, Value>>,
    pub bases: Vec<Rc<Class>>,
    pub field_defs: Vec<ClassFieldDef>,
    pub is_dataclass: bool,
    pub is_enum_base: bool,
    pub is_int_enum_base: bool,
}

#[derive(Clone)]
pub struct BoundMethod {
    pub instance: Rc<RefCell<Instance>>,
    pub function: Rc<Function>,
    pub owner_class: Rc<Class>,
}

#[derive(Clone)]
pub struct BoundClassMethod {
    pub class: Rc<Class>,
    pub function: Rc<Function>,
}

#[derive(Clone)]
pub struct Instance {
    pub class: Rc<Class>,
    pub fields: HashMap<String, Value>,
}

#[derive(Clone)]
pub struct Module {
    pub name: String,
    pub attrs: HashMap<String, Value>,
}

#[derive(Clone)]
pub struct SuperObject {
    pub instance: Rc<RefCell<Instance>>,
    pub class: Rc<Class>,
}

#[derive(Clone)]
pub struct FieldSpecValue {
    pub has_default: bool,
    pub default: Option<Value>,
    pub default_factory: Option<Value>,
}

#[derive(Clone)]
pub struct ClassFieldDef {
    pub name: String,
    pub has_default: bool,
    pub default: Option<Value>,
}

#[derive(Clone)]
pub struct RangeValue {
    pub start: i64,
    pub stop: i64,
    pub step: i64,
}

#[derive(Clone)]
pub struct Generator {
    pub function: Rc<Function>,
    pub call_env: Env,
    pub computed: bool,
    pub values: Vec<Value>,
    pub index: usize,
}

pub struct FileHandle {
    pub file: File,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Key {
    None,
    Bool(bool),
    Int(i64),
    Str(String),
    Tuple(Vec<Key>),
}

#[derive(Copy, Clone)]
pub enum FileMethod {
    Read,
    Write,
    Close,
    Enter,
    Exit,
}

#[derive(Copy, Clone)]
pub enum ListMethod {
    Append,
    Extend,
    Pop,
    Remove,
    Sort,
    Copy,
}

#[derive(Copy, Clone)]
pub enum DictMethod {
    Get,
    Keys,
    Values,
    Items,
    SetDefault,
    Copy,
}

#[derive(Copy, Clone)]
pub enum SetMethod {
    Add,
    Update,
    Get,
    Remove,
    Copy,
}

#[derive(Copy, Clone)]
pub enum StringMethod {
    StartsWith,
    EndsWith,
    IsAlpha,
    IsAlnum,
    IsDigit,
    IsLower,
    IsUpper,
    IsSpace,
    Split,
    Strip,
    LStrip,
    RStrip,
    Lower,
    Upper,
    Capitalize,
    Replace,
    Join,
    Format,
    Encode,
}

#[derive(Copy, Clone)]
pub enum BytesMethod {
    Decode,
}

#[derive(Copy, Clone)]
pub enum PathMethod {
    Resolve,
    Exists,
    IsFile,
    IsDir,
    ReadText,
    WriteText,
    Open,
    Mkdir,
    RelativeTo,
}

#[derive(Copy, Clone)]
pub enum NamespaceMethod {
    None,
}

#[derive(Copy, Clone)]
pub enum ArgParserMethod {
    AddArgument,
    AddMutuallyExclusiveGroup,
    ParseArgs,
}

#[derive(Clone)]
pub struct ArgParser {
    pub specs: Vec<ArgSpec>,
}

#[derive(Clone)]
pub struct ArgSpec {
    pub names: Vec<String>,
    pub dest: String,
    pub positional: bool,
    pub action: ArgAction,
    pub nargs: ArgNargs,
    pub arg_type: Option<Value>,
    pub default: Value,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum ArgAction {
    Store,
    StoreTrue,
    Append,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum ArgNargs {
    One,
    Star,
}

#[derive(Clone)]
enum ExecMode {
    Normal,
    CollectYield(Rc<RefCell<Vec<Value>>>),
}

enum Flow {
    Next,
    Return(Value),
    Break,
    Continue,
}

pub struct Interpreter {
    modules: HashMap<String, Value>,
    blocked_roots: HashSet<String>,
    search_paths: Vec<PathBuf>,
    argv: Vec<String>,
    call_stack: Vec<Env>,
}

impl Interpreter {
    pub fn new() -> Self {
        let blocked_roots = [
            "asyncio",
            "threading",
            "multiprocessing",
            "pickle",
            "socket",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        Self {
            modules: HashMap::new(),
            blocked_roots,
            search_paths: vec![PathBuf::from(".")],
            argv: Vec::new(),
            call_stack: Vec::new(),
        }
    }

    pub fn set_search_paths(&mut self, paths: Vec<PathBuf>) {
        self.search_paths = paths;
    }

    pub fn set_argv(&mut self, argv: Vec<String>) {
        self.argv = argv;
    }

    pub fn run_program(&mut self, program: &Program, module_name: &str) -> RtResult<Env> {
        let env = Env::new(None);
        self.install_builtins(&env);
        env.set_local("__name__", Value::Str(module_name.to_owned()));
        self.exec_block(&program.body, env.clone(), &ExecMode::Normal)?;
        Ok(env)
    }

    pub fn run_source(&mut self, source: &str, module_name: &str) -> RtResult<Env> {
        let tokens = lexer::lex(source).map_err(|err| format!("{module_name}: {err}"))?;
        let program = parser::parse(tokens).map_err(|err| format!("{module_name}: {err}"))?;
        self.run_program(&program, module_name)
    }

    pub fn run_module(&mut self, module_name: &str) -> RtResult<()> {
        if let Some(entry_path) = resolve_module_entry_path(module_name, &self.search_paths) {
            let argv = if self.argv.is_empty() {
                vec![module_name.to_owned()]
            } else {
                self.argv.clone()
            };
            let _ = self.call_main(&entry_path, &argv)?;
            return Ok(());
        }
        let _ = self.import_module(module_name)?;
        Ok(())
    }

    pub fn call_main(&mut self, script_path: &Path, argv: &[String]) -> RtResult<i32> {
        let source = std::fs::read_to_string(script_path).map_err(|err| {
            format!(
                "failed to read script '{}': {err}",
                script_path.to_string_lossy()
            )
        })?;

        let mut paths = Vec::new();
        if let Some(parent) = script_path.parent() {
            paths.push(parent.to_path_buf());
            let is_package_main = script_path.file_name().is_some_and(|n| n == "__main__.py")
                && parent.join("__init__.py").is_file();
            if is_package_main {
                if let Some(grand_parent) = parent.parent() {
                    paths.push(grand_parent.to_path_buf());
                }
            }
        }
        paths.extend(self.search_paths.clone());
        self.search_paths = paths;
        self.argv = argv.to_vec();

        let env = self.run_source(&source, "__main__")?;
        let argv_list = argv.iter().cloned().map(Value::Str).collect::<Vec<_>>();
        env.set_local("argv", Value::List(Rc::new(RefCell::new(argv_list))));
        Ok(0)
    }

    fn install_builtins(&mut self, env: &Env) {
        env.set_local("None", Value::None);
        env.set_local("True", Value::Bool(true));
        env.set_local("False", Value::Bool(false));
        env.set_local("print", Value::Builtin(Builtin::Native(builtin_print)));
        env.set_local("len", Value::Builtin(Builtin::Native(builtin_len)));
        env.set_local("range", Value::Builtin(Builtin::Native(builtin_range)));
        env.set_local("open", Value::Builtin(Builtin::Native(builtin_open)));
        env.set_local("list", Value::Builtin(Builtin::Native(builtin_list)));
        env.set_local("dict", Value::Builtin(Builtin::Native(builtin_dict)));
        env.set_local("set", Value::Builtin(Builtin::Native(builtin_set)));
        env.set_local("str", Value::Builtin(Builtin::Native(builtin_str)));
        env.set_local("repr", Value::Builtin(Builtin::Native(builtin_repr)));
        env.set_local("int", Value::Builtin(Builtin::Native(builtin_int)));
        env.set_local("bool", Value::Builtin(Builtin::Native(builtin_bool)));
        env.set_local("float", Value::Builtin(Builtin::Native(builtin_float)));
        env.set_local("type", Value::Builtin(Builtin::Native(builtin_type)));
        env.set_local("hasattr", Value::Builtin(Builtin::Native(builtin_hasattr)));
        env.set_local("getattr", Value::Builtin(Builtin::Native(builtin_getattr)));
        env.set_local("setattr", Value::Builtin(Builtin::Native(builtin_setattr)));
        env.set_local(
            "isinstance",
            Value::Builtin(Builtin::Native(builtin_isinstance)),
        );
        env.set_local("enumerate", Value::Builtin(Builtin::Native(builtin_enumerate)));
        env.set_local("zip", Value::Builtin(Builtin::Native(builtin_zip)));
        env.set_local("sorted", Value::Builtin(Builtin::Native(builtin_sorted)));
        env.set_local("reversed", Value::Builtin(Builtin::Native(builtin_reversed)));
        env.set_local("min", Value::Builtin(Builtin::Native(builtin_min)));
        env.set_local("max", Value::Builtin(Builtin::Native(builtin_max)));
        env.set_local("any", Value::Builtin(Builtin::Native(builtin_any)));
        env.set_local("all", Value::Builtin(Builtin::Native(builtin_all)));
        env.set_local("super", Value::Builtin(Builtin::Native(builtin_super)));
        env.set_local(
            "classmethod",
            Value::Builtin(Builtin::Native(builtin_classmethod)),
        );
        env.set_local(
            "staticmethod",
            Value::Builtin(Builtin::Native(builtin_staticmethod)),
        );
        env.set_local("property", Value::Builtin(Builtin::Native(builtin_property)));

        let mut object_attrs = HashMap::new();
        object_attrs.insert(
            "__init__".to_owned(),
            Value::Function(Rc::new(Function {
                name: "__init__".to_owned(),
                params: vec![
                    Param {
                        name: "self".to_owned(),
                        default: None,
                        kind: ParamKind::Positional,
                    },
                    Param {
                        name: "args".to_owned(),
                        default: None,
                        kind: ParamKind::VarArgs,
                    },
                ],
                body: Vec::new(),
                env: env.clone(),
                is_generator: false,
            })),
        );
        let object_class = Rc::new(Class {
            name: "object".to_owned(),
            attrs: RefCell::new(object_attrs),
            bases: Vec::new(),
            field_defs: Vec::new(),
            is_dataclass: false,
            is_enum_base: false,
            is_int_enum_base: false,
        });
        let exception_class = Rc::new(Class {
            name: "Exception".to_owned(),
            attrs: RefCell::new(HashMap::new()),
            bases: vec![object_class.clone()],
            field_defs: Vec::new(),
            is_dataclass: false,
            is_enum_base: false,
            is_int_enum_base: false,
        });
        let value_error_class = Rc::new(Class {
            name: "ValueError".to_owned(),
            attrs: RefCell::new(HashMap::new()),
            bases: vec![exception_class.clone()],
            field_defs: Vec::new(),
            is_dataclass: false,
            is_enum_base: false,
            is_int_enum_base: false,
        });
        let type_error_class = Rc::new(Class {
            name: "TypeError".to_owned(),
            attrs: RefCell::new(HashMap::new()),
            bases: vec![exception_class.clone()],
            field_defs: Vec::new(),
            is_dataclass: false,
            is_enum_base: false,
            is_int_enum_base: false,
        });
        let runtime_error_class = Rc::new(Class {
            name: "RuntimeError".to_owned(),
            attrs: RefCell::new(HashMap::new()),
            bases: vec![exception_class.clone()],
            field_defs: Vec::new(),
            is_dataclass: false,
            is_enum_base: false,
            is_int_enum_base: false,
        });
        env.set_local("object", Value::Class(object_class));
        env.set_local("Exception", Value::Class(exception_class));
        env.set_local("ValueError", Value::Class(value_error_class));
        env.set_local("TypeError", Value::Class(type_error_class));
        env.set_local("RuntimeError", Value::Class(runtime_error_class));
    }

    fn exec_block(&mut self, body: &[Stmt], env: Env, mode: &ExecMode) -> RtResult<Flow> {
        for stmt in body {
            let flow = self.exec_stmt(stmt, env.clone(), mode)?;
            match flow {
                Flow::Next => {}
                Flow::Return(_) | Flow::Break | Flow::Continue => return Ok(flow),
            }
        }
        Ok(Flow::Next)
    }

    fn exec_stmt(&mut self, stmt: &Stmt, env: Env, mode: &ExecMode) -> RtResult<Flow> {
        match stmt {
            Stmt::Expr(expr) => {
                self.eval_expr(expr, env, mode)?;
                Ok(Flow::Next)
            }
            Stmt::Assign { target, value } => {
                let rhs = self.eval_expr(value, env.clone(), mode)?;
                self.assign_target(target, rhs, env, mode)?;
                Ok(Flow::Next)
            }
            Stmt::AnnAssign { target, value } => {
                if let Some(expr) = value {
                    let rhs = self.eval_expr(expr, env.clone(), mode)?;
                    self.assign_target(target, rhs, env, mode)?;
                }
                Ok(Flow::Next)
            }
            Stmt::If { test, body, orelse } => {
                let condition = self.eval_expr(test, env.clone(), mode)?;
                if self.is_truthy(&condition) {
                    self.exec_block(body, env.clone(), mode)
                } else if !orelse.is_empty() {
                    self.exec_block(orelse, env.clone(), mode)
                } else {
                    Ok(Flow::Next)
                }
            }
            Stmt::While { test, body } => {
                loop {
                    let condition = self.eval_expr(test, env.clone(), mode)?;
                    if !self.is_truthy(&condition) {
                        break;
                    }
                    match self.exec_block(body, env.clone(), mode)? {
                        Flow::Next => {}
                        Flow::Continue => continue,
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
                Ok(Flow::Next)
            }
            Stmt::For { target, iter, body } => {
                let iterable = self.eval_expr(iter, env.clone(), mode)?;
                for item in self.collect_iterable(iterable)? {
                    self.assign_target(target, item, env.clone(), mode)?;
                    match self.exec_block(body, env.clone(), mode)? {
                        Flow::Next => {}
                        Flow::Continue => continue,
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
                Ok(Flow::Next)
            }
            Stmt::FunctionDef {
                name,
                decorators,
                params,
                body,
            } => {
                let function = Function {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    env: env.clone(),
                    is_generator: contains_yield(body),
                };
                let mut value = Value::Function(Rc::new(function));
                for deco in decorators.iter().rev() {
                    let deco_value = self.eval_expr(deco, env.clone(), mode)?;
                    value = self.call_callable(deco_value, vec![value], HashMap::new())?;
                }
                env.set_local(name.clone(), value);
                Ok(Flow::Next)
            }
            Stmt::ClassDef {
                name,
                decorators,
                bases,
                body,
            } => {
                let mut base_classes = Vec::<Rc<Class>>::new();
                for base in bases {
                    let value = self.eval_expr(base, env.clone(), mode)?;
                    if let Value::Class(class) = value {
                        base_classes.push(class);
                    } else {
                        return Err(format!("class base for '{name}' is not a class"));
                    }
                }
                let class_env = Env::new(Some(env.clone()));
                self.install_builtins(&class_env);
                let mut field_defs = Vec::<ClassFieldDef>::new();
                let mut attr_order = Vec::<String>::new();
                for stmt in body {
                    match stmt {
                        Stmt::AnnAssign { target, value } => {
                            if let Expr::Name(name) = target {
                                attr_order.push(name.clone());
                                if let Some(expr) = value {
                                    let rhs = self.eval_expr(expr, class_env.clone(), mode)?;
                                    class_env.set_local(name.clone(), rhs.clone());
                                    field_defs.push(ClassFieldDef {
                                        name: name.clone(),
                                        has_default: true,
                                        default: Some(rhs),
                                    });
                                } else {
                                    field_defs.push(ClassFieldDef {
                                        name: name.clone(),
                                        has_default: false,
                                        default: None,
                                    });
                                }
                                continue;
                            }
                            let flow = self.exec_stmt(stmt, class_env.clone(), mode)?;
                            if !matches!(flow, Flow::Next) {
                                return Err("invalid control flow inside class body".to_owned());
                            }
                        }
                        Stmt::Assign { target, .. } => {
                            if let Expr::Name(attr_name) = target {
                                attr_order.push(attr_name.clone());
                            }
                            let flow = self.exec_stmt(stmt, class_env.clone(), mode)?;
                            if !matches!(flow, Flow::Next) {
                                return Err("invalid control flow inside class body".to_owned());
                            }
                        }
                        Stmt::FunctionDef { name, .. } => {
                            attr_order.push(name.clone());
                            let flow = self.exec_stmt(stmt, class_env.clone(), mode)?;
                            if !matches!(flow, Flow::Next) {
                                return Err("invalid control flow inside class body".to_owned());
                            }
                        }
                        _ => {
                            let flow = self.exec_stmt(stmt, class_env.clone(), mode)?;
                            if !matches!(flow, Flow::Next) {
                                return Err("invalid control flow inside class body".to_owned());
                            }
                        }
                    }
                }
                let attrs = class_env.values_snapshot();
                let class = Rc::new(Class {
                    name: name.clone(),
                    attrs: RefCell::new(attrs),
                    bases: base_classes,
                    field_defs,
                    is_dataclass: false,
                    is_enum_base: false,
                    is_int_enum_base: false,
                });
                finalize_enum_class(class.clone(), &attr_order)?;
                let mut class_value = Value::Class(class);
                for deco in decorators.iter().rev() {
                    let deco_value = self.eval_expr(deco, env.clone(), mode)?;
                    class_value = self.call_callable(deco_value, vec![class_value], HashMap::new())?;
                }
                env.set_local(name.clone(), class_value);
                Ok(Flow::Next)
            }
            Stmt::Return(value) => {
                let v = if let Some(expr) = value {
                    self.eval_expr(expr, env, mode)?
                } else {
                    Value::None
                };
                Ok(Flow::Return(v))
            }
            Stmt::Raise(value) => {
                let msg = if let Some(expr) = value {
                    let val = self.eval_expr(expr, env, mode)?;
                    value_to_string(&val)
                } else {
                    "raise".to_owned()
                };
                Err(msg)
            }
            Stmt::Break => Ok(Flow::Break),
            Stmt::Continue => Ok(Flow::Continue),
            Stmt::Pass => Ok(Flow::Next),
            Stmt::Import(names) => {
                for name in names {
                    let root = name.split('.').next().unwrap_or(name);
                    let _ = self.import_module(name)?;
                    let root_module = self.import_module(root)?;
                    env.set_local(root.to_owned(), root_module);
                }
                Ok(Flow::Next)
            }
            Stmt::FromImport { module, names } => {
                let module_value = self.import_module(module)?;
                for item in names {
                    let value = self
                        .get_attr(&module_value, &item.name)?
                        .ok_or_else(|| format!("module '{module}' has no attribute '{}'", item.name))?;
                    let bind_name = item.asname.as_ref().unwrap_or(&item.name);
                    env.set_local(bind_name.clone(), value);
                }
                Ok(Flow::Next)
            }
            Stmt::AugAssign { target, op, value } => {
                let lhs = self.eval_expr(target, env.clone(), mode)?;
                let rhs = self.eval_expr(value, env.clone(), mode)?;
                let out = self.eval_binary(op, lhs, rhs)?;
                self.assign_target(target, out, env, mode)?;
                Ok(Flow::Next)
            }
            Stmt::With {
                context,
                asname,
                body,
            } => {
                let context_value = self.eval_expr(context, env.clone(), mode)?;
                let enter = self
                    .get_attr(&context_value, "__enter__")?
                    .ok_or_else(|| "context object has no __enter__".to_owned())?;
                let entered = self.call_callable(enter, Vec::new(), HashMap::new())?;
                if let Some(name) = asname {
                    env.set_local(name.clone(), entered);
                }
                let body_flow = self.exec_block(body, env.clone(), mode)?;
                if let Some(exit) = self.get_attr(&context_value, "__exit__")? {
                    let _ = self.call_callable(
                        exit,
                        vec![Value::None, Value::None, Value::None],
                        HashMap::new(),
                    )?;
                }
                Ok(body_flow)
            }
            Stmt::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                let result = match self.exec_block(body, env.clone(), mode) {
                    Ok(flow) => {
                        if matches!(flow, Flow::Next) && !orelse.is_empty() {
                            self.exec_block(orelse, env.clone(), mode)
                        } else {
                            Ok(flow)
                        }
                    }
                    Err(err) => {
                        if handlers.is_empty() {
                            Err(err)
                        } else {
                            let mut handled_flow = Flow::Next;
                            let mut handled = false;
                            for handler in handlers {
                                if let Some(name) = &handler.exc_name {
                                    let mut ex = HashMap::new();
                                    ex.insert("message".to_owned(), Value::Str(err.clone()));
                                    ex.insert("line".to_owned(), Value::Int(0));
                                    ex.insert("column".to_owned(), Value::Int(0));
                                    ex.insert("file".to_owned(), Value::Str(String::new()));
                                    env.set_local(name.clone(), Value::Namespace(Rc::new(RefCell::new(ex))));
                                }
                                handled_flow = self.exec_block(&handler.body, env.clone(), mode)?;
                                handled = true;
                                break;
                            }
                            if handled {
                                Ok(handled_flow)
                            } else {
                                Err(err)
                            }
                        }
                    }
                };

                if !finalbody.is_empty() {
                    let finally_flow = self.exec_block(finalbody, env, mode)?;
                    if !matches!(finally_flow, Flow::Next) {
                        return Ok(finally_flow);
                    }
                }
                result
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr, env: Env, mode: &ExecMode) -> RtResult<Value> {
        match expr {
            Expr::Name(name) => env
                .get(name)
                .ok_or_else(|| format!("name '{name}' is not defined")),
            Expr::Int(v) => Ok(Value::Int(*v)),
            Expr::Str(v) => Ok(Value::Str(v.clone())),
            Expr::FStr(v) => Ok(Value::Str(self.eval_fstring(v, env, mode)?)),
            Expr::Bool(v) => Ok(Value::Bool(*v)),
            Expr::None => Ok(Value::None),
            Expr::List(values) => {
                let mut out = Vec::new();
                for value in values {
                    if let Expr::Starred(inner) = value {
                        let expanded = self.eval_expr(inner, env.clone(), mode)?;
                        out.extend(self.collect_iterable(expanded)?);
                    } else {
                        out.push(self.eval_expr(value, env.clone(), mode)?);
                    }
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            Expr::ListComp {
                elem,
                target,
                iter,
                cond,
            } => {
                let iterable = self.eval_expr(iter, env.clone(), mode)?;
                let mut out = Vec::new();
                for item in self.collect_iterable(iterable)? {
                    self.assign_target(target, item, env.clone(), mode)?;
                    if let Some(cond_expr) = cond {
                        let keep = self.eval_expr(cond_expr, env.clone(), mode)?;
                        if !self.is_truthy(&keep) {
                            continue;
                        }
                    }
                    out.push(self.eval_expr(elem, env.clone(), mode)?);
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            Expr::Dict(items) => {
                let mut out = BTreeMap::new();
                for (k, v) in items {
                    let key = key_from_value(&self.eval_expr(k, env.clone(), mode)?)?;
                    out.insert(key, self.eval_expr(v, env.clone(), mode)?);
                }
                Ok(Value::Dict(Rc::new(RefCell::new(out))))
            }
            Expr::DictComp {
                key,
                value,
                target,
                iter,
                cond,
            } => {
                let iterable = self.eval_expr(iter, env.clone(), mode)?;
                let mut out = BTreeMap::new();
                for item in self.collect_iterable(iterable)? {
                    self.assign_target(target, item, env.clone(), mode)?;
                    if let Some(cond_expr) = cond {
                        let keep = self.eval_expr(cond_expr, env.clone(), mode)?;
                        if !self.is_truthy(&keep) {
                            continue;
                        }
                    }
                    let k = key_from_value(&self.eval_expr(key, env.clone(), mode)?)?;
                    let v = self.eval_expr(value, env.clone(), mode)?;
                    out.insert(k, v);
                }
                Ok(Value::Dict(Rc::new(RefCell::new(out))))
            }
            Expr::Set(items) => {
                let mut out = BTreeSet::new();
                for item in items {
                    if let Expr::Starred(inner) = item {
                        let expanded = self.eval_expr(inner, env.clone(), mode)?;
                        for value in self.collect_iterable(expanded)? {
                            out.insert(key_from_value(&value)?);
                        }
                    } else {
                        let value = self.eval_expr(item, env.clone(), mode)?;
                        out.insert(key_from_value(&value)?);
                    }
                }
                Ok(Value::Set(Rc::new(RefCell::new(out))))
            }
            Expr::SetComp {
                elem,
                target,
                iter,
                cond,
            } => {
                let iterable = self.eval_expr(iter, env.clone(), mode)?;
                let mut out = BTreeSet::new();
                for item in self.collect_iterable(iterable)? {
                    self.assign_target(target, item, env.clone(), mode)?;
                    if let Some(cond_expr) = cond {
                        let keep = self.eval_expr(cond_expr, env.clone(), mode)?;
                        if !self.is_truthy(&keep) {
                            continue;
                        }
                    }
                    let v = self.eval_expr(elem, env.clone(), mode)?;
                    out.insert(key_from_value(&v)?);
                }
                Ok(Value::Set(Rc::new(RefCell::new(out))))
            }
            Expr::GenComp {
                elem,
                target,
                iter,
                cond,
            } => {
                // Phase 1: represent generator comprehensions as eager lists.
                let iterable = self.eval_expr(iter, env.clone(), mode)?;
                let mut out = Vec::new();
                for item in self.collect_iterable(iterable)? {
                    self.assign_target(target, item, env.clone(), mode)?;
                    if let Some(cond_expr) = cond {
                        let keep = self.eval_expr(cond_expr, env.clone(), mode)?;
                        if !self.is_truthy(&keep) {
                            continue;
                        }
                    }
                    out.push(self.eval_expr(elem, env.clone(), mode)?);
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            Expr::Unary { op, expr } => {
                let value = self.eval_expr(expr, env, mode)?;
                match op {
                    UnaryOp::Neg => match value {
                        Value::Int(v) => Ok(Value::Int(-v)),
                        _ => Err("unary '-' expects int".to_owned()),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!self.is_truthy(&value))),
                }
            }
            Expr::Binary { left, op, right } => {
                if *op == BinaryOp::And {
                    let left_value = self.eval_expr(left, env.clone(), mode)?;
                    if !self.is_truthy(&left_value) {
                        return Ok(left_value);
                    }
                    return self.eval_expr(right, env, mode);
                }
                if *op == BinaryOp::Or {
                    let left_value = self.eval_expr(left, env.clone(), mode)?;
                    if self.is_truthy(&left_value) {
                        return Ok(left_value);
                    }
                    return self.eval_expr(right, env, mode);
                }

                let left_value = self.eval_expr(left, env.clone(), mode)?;
                let right_value = self.eval_expr(right, env, mode)?;
                self.eval_binary(op, left_value, right_value)
            }
            Expr::IfExpr {
                then_expr,
                condition,
                else_expr,
            } => {
                let cond_value = self.eval_expr(condition, env.clone(), mode)?;
                if self.is_truthy(&cond_value) {
                    self.eval_expr(then_expr, env, mode)
                } else {
                    self.eval_expr(else_expr, env, mode)
                }
            }
            Expr::Lambda { params, body } => Ok(Value::Function(Rc::new(Function {
                name: "<lambda>".to_owned(),
                params: params.clone(),
                body: vec![Stmt::Return(Some((**body).clone()))],
                env,
                is_generator: false,
            }))),
            Expr::Call { func, args, kwargs } => {
                let callee = self.eval_expr(func, env.clone(), mode)?;
                let mut values = Vec::new();
                for arg in args {
                    values.push(self.eval_expr(arg, env.clone(), mode)?);
                }
                let mut kw_values = HashMap::new();
                for (name, value_expr) in kwargs {
                    let value = self.eval_expr(value_expr, env.clone(), mode)?;
                    kw_values.insert(name.clone(), value);
                }
                self.call_callable(callee, values, kw_values)
            }
            Expr::Attr { value, name } => {
                let obj = self.eval_expr(value, env, mode)?;
                self.get_attr(&obj, name)?
                    .ok_or_else(|| format!("attribute '{name}' not found on {}", display_value(&obj)))
            }
            Expr::Subscript { value, index } => {
                let base = self.eval_expr(value, env.clone(), mode)?;
                let idx = self.eval_expr(index, env, mode)?;
                self.get_item(base, idx)
            }
            Expr::Slice { start, stop } => {
                let start_value = if let Some(s) = start {
                    self.eval_expr(s, env.clone(), mode)?
                } else {
                    Value::None
                };
                let stop_value = if let Some(s) = stop {
                    self.eval_expr(s, env, mode)?
                } else {
                    Value::None
                };
                Ok(Value::List(Rc::new(RefCell::new(vec![start_value, stop_value]))))
            }
            Expr::Yield(value) => match mode {
                ExecMode::Normal => Err("yield outside generator".to_owned()),
                ExecMode::CollectYield(out) => {
                    let yielded = if let Some(inner) = value {
                        self.eval_expr(inner, env, mode)?
                    } else {
                        Value::None
                    };
                    out.borrow_mut().push(yielded);
                    Ok(Value::None)
                }
            },
            Expr::Starred(inner) => self.eval_expr(inner, env, mode),
        }
    }

    fn eval_fstring(&mut self, template: &str, env: Env, mode: &ExecMode) -> RtResult<String> {
        let chars: Vec<char> = template.chars().collect();
        let mut out = String::new();
        let mut i = 0usize;
        while i < chars.len() {
            match chars[i] {
                '{' => {
                    if i + 1 < chars.len() && chars[i + 1] == '{' {
                        out.push('{');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    let start = i;
                    let mut depth = 0isize;
                    let mut in_string = None::<char>;
                    while i < chars.len() {
                        let ch = chars[i];
                        if let Some(quote) = in_string {
                            if ch == '\\' {
                                i += 2;
                                continue;
                            }
                            if ch == quote {
                                in_string = None;
                            }
                            i += 1;
                            continue;
                        }
                        if ch == '\'' || ch == '"' {
                            in_string = Some(ch);
                            i += 1;
                            continue;
                        }
                        if ch == '{' {
                            depth += 1;
                            i += 1;
                            continue;
                        }
                        if ch == '}' {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                        }
                        i += 1;
                    }
                    if i >= chars.len() {
                        return Err(format!(
                            "unterminated f-string expression in {:?}",
                            template
                        ));
                    }
                    let raw_expr: String = chars[start..i].iter().collect();
                    let mut expr_text = raw_expr.trim().to_owned();
                    let mut cut = None::<usize>;
                    let mut inner_depth = 0isize;
                    for (idx, ch) in expr_text.char_indices() {
                        match ch {
                            '(' | '[' | '{' => inner_depth += 1,
                            ')' | ']' | '}' => {
                                if inner_depth > 0 {
                                    inner_depth -= 1;
                                }
                            }
                            '!' | ':' if inner_depth == 0 => {
                                cut = Some(idx);
                                break;
                            }
                            _ => {}
                        }
                    }
                    if let Some(idx) = cut {
                        expr_text.truncate(idx);
                        expr_text = expr_text.trim().to_owned();
                    }
                    let tokens = lexer::lex(&expr_text)
                        .map_err(|err| format!("invalid f-string expression: {err}"))?;
                    let expr = parser::parse_expression(tokens)
                        .map_err(|err| format!("invalid f-string expression: {err}"))?;
                    let value = self.eval_expr(&expr, env.clone(), mode)?;
                    out.push_str(&value_to_string(&value));
                    i += 1;
                }
                '}' => {
                    if i + 1 < chars.len() && chars[i + 1] == '}' {
                        out.push('}');
                        i += 2;
                    } else {
                        return Err(format!("single '}}' in f-string {:?}", template));
                    }
                }
                ch => {
                    out.push(ch);
                    i += 1;
                }
            }
        }
        Ok(out)
    }

    fn eval_binary(&self, op: &BinaryOp, left: Value, right: Value) -> RtResult<Value> {
        match op {
            BinaryOp::Add => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Str(a + &b)),
                (Value::List(a), Value::List(b)) => {
                    let mut out = a.borrow().clone();
                    out.extend(b.borrow().iter().cloned());
                    Ok(Value::List(Rc::new(RefCell::new(out))))
                }
                _ => Err("unsupported '+' operands".to_owned()),
            },
            BinaryOp::Sub => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
                _ => Err("unsupported '-' operands".to_owned()),
            },
            BinaryOp::Mul => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                (Value::Str(a), Value::Int(times)) => {
                    if times < 0 {
                        return Ok(Value::Str(String::new()));
                    }
                    Ok(Value::Str(a.repeat(times as usize)))
                }
                _ => Err("unsupported '*' operands".to_owned()),
            },
            BinaryOp::Div => match (left, right) {
                (Value::Int(_), Value::Int(0)) => Err("division by zero".to_owned()),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
                (Value::Path(mut p), Value::Str(seg)) => {
                    p.push(seg);
                    Ok(Value::Path(p))
                }
                (Value::Path(mut p), Value::Path(seg)) => {
                    p.push(seg);
                    Ok(Value::Path(p))
                }
                _ => Err("unsupported '/' operands".to_owned()),
            },
            BinaryOp::Mod => match (left, right) {
                (Value::Int(_), Value::Int(0)) => Err("modulo by zero".to_owned()),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
                _ => Err("unsupported '%' operands".to_owned()),
            },
            BinaryOp::BitAnd => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
                _ => Err("unsupported '&' operands".to_owned()),
            },
            BinaryOp::BitOr => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
                _ => Err("unsupported '|' operands".to_owned()),
            },
            BinaryOp::BitXor => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
                _ => Err("unsupported '^' operands".to_owned()),
            },
            BinaryOp::LShift => match (left, right) {
                (Value::Int(a), Value::Int(b)) if b >= 0 => Ok(Value::Int(a << (b as u32))),
                _ => Err("unsupported '<<' operands".to_owned()),
            },
            BinaryOp::RShift => match (left, right) {
                (Value::Int(a), Value::Int(b)) if b >= 0 => Ok(Value::Int(a >> (b as u32))),
                _ => Err("unsupported '>>' operands".to_owned()),
            },
            BinaryOp::Eq => Ok(Value::Bool(value_eq(&left, &right))),
            BinaryOp::Ne => Ok(Value::Bool(!value_eq(&left, &right))),
            BinaryOp::Lt => eval_compare(left, right, |o| o == std::cmp::Ordering::Less),
            BinaryOp::Lte => eval_compare(left, right, |o| {
                o == std::cmp::Ordering::Less || o == std::cmp::Ordering::Equal
            }),
            BinaryOp::Gt => eval_compare(left, right, |o| o == std::cmp::Ordering::Greater),
            BinaryOp::Gte => eval_compare(left, right, |o| {
                o == std::cmp::Ordering::Greater || o == std::cmp::Ordering::Equal
            }),
            BinaryOp::In => eval_contains(left, right),
            BinaryOp::NotIn => {
                let contains = eval_contains(left, right)?;
                match contains {
                    Value::Bool(v) => Ok(Value::Bool(!v)),
                    _ => Err("invalid not in result".to_owned()),
                }
            }
            BinaryOp::And | BinaryOp::Or => unreachable!(),
        }
    }

    fn assign_target(
        &mut self,
        target: &Expr,
        value: Value,
        env: Env,
        mode: &ExecMode,
    ) -> RtResult<()> {
        match target {
            Expr::Name(name) => {
                env.set_local(name.clone(), value);
                Ok(())
            }
            Expr::List(items) => {
                let unpacked = self.collect_iterable(value)?;
                if unpacked.len() != items.len() {
                    return Err("unpack mismatch in assignment target".to_owned());
                }
                for (target_expr, item) in items.iter().zip(unpacked.into_iter()) {
                    self.assign_target(target_expr, item, env.clone(), mode)?;
                }
                Ok(())
            }
            Expr::Attr {
                value: obj_expr,
                name,
            } => {
                let obj = self.eval_expr(obj_expr, env, mode)?;
                self.set_attr(obj, name, value)
            }
            Expr::Subscript {
                value: obj_expr,
                index,
            } => {
                let obj = self.eval_expr(obj_expr, env.clone(), mode)?;
                let idx = self.eval_expr(index, env, mode)?;
                self.set_item(obj, idx, value)
            }
            _ => Err("unsupported assignment target".to_owned()),
        }
    }

    fn call_callable(
        &mut self,
        callable: Value,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        match callable {
            Value::Builtin(Builtin::Native(f)) => f(self, args, kwargs),
            Value::Builtin(Builtin::BoundFile { file, method }) => {
                self.call_file_method(file, method, args, kwargs)
            }
            Value::Builtin(Builtin::BoundList { list, method }) => {
                self.call_list_method(list, method, args, kwargs)
            }
            Value::Builtin(Builtin::BoundDict { dict, method }) => {
                self.call_dict_method(dict, method, args, kwargs)
            }
            Value::Builtin(Builtin::BoundSet { set, method }) => {
                self.call_set_method(set, method, args, kwargs)
            }
            Value::Builtin(Builtin::BoundString { value, method }) => {
                self.call_string_method(value, method, args, kwargs)
            }
            Value::Builtin(Builtin::BoundBytes { value, method }) => {
                self.call_bytes_method(value, method, args, kwargs)
            }
            Value::Builtin(Builtin::BoundPath { value, method }) => {
                self.call_path_method(value, method, args, kwargs)
            }
            Value::Builtin(Builtin::BoundNamespace { value, method }) => {
                self.call_namespace_method(value, method, args, kwargs)
            }
            Value::Builtin(Builtin::BoundArgParser { value, method }) => {
                self.call_arg_parser_method(value, method, args, kwargs)
            }
            Value::Function(function) => self.call_function(function, None, None, args, kwargs),
            Value::BoundMethod(bound) => self.call_function(
                bound.function.clone(),
                Some(bound.instance.clone()),
                Some(bound.owner_class.clone()),
                args,
                kwargs,
            ),
            Value::BoundClassMethod(bound) => {
                let mut full_args = Vec::with_capacity(args.len() + 1);
                full_args.push(Value::Class(bound.class.clone()));
                full_args.extend(args);
                self.call_function(bound.function.clone(), None, None, full_args, kwargs)
            }
            Value::Class(class) => self.call_class(class, args, kwargs),
            Value::ArgParser(parser) => self.call_arg_parser_method(
                parser,
                ArgParserMethod::ParseArgs,
                args,
                kwargs,
            ),
            _ => Err(format!(
                "object '{}' is not callable",
                display_value(&callable)
            )),
        }
    }

    fn call_function(
        &mut self,
        function: Rc<Function>,
        bound_self: Option<Rc<RefCell<Instance>>>,
        owner_class: Option<Rc<Class>>,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        let call_env = Env::new(Some(function.env.clone()));
        let mut pos_index = 0usize;

        if let Some(instance) = bound_self {
            if function.params.is_empty() {
                return Err("bound method has no self parameter".to_owned());
            }
            call_env.set_local(
                function.params[0].name.clone(),
                Value::Instance(instance.clone()),
            );
            call_env.set_local("__self__", Value::Instance(instance));
            if let Some(owner) = owner_class {
                call_env.set_local("__class__", Value::Class(owner));
            }
            pos_index = 1;
        }

        let mut arg_index = 0usize;
        let mut consumed_kwargs = HashSet::<String>::new();
        let mut positional_param_names = HashSet::<String>::new();
        let mut bound_by_position = HashSet::<String>::new();
        let mut var_kwargs_name = None::<String>;

        for param in function.params.iter().skip(pos_index) {
            match param.kind {
                ParamKind::Positional => {
                    positional_param_names.insert(param.name.clone());
                    if arg_index < args.len() {
                        call_env.set_local(param.name.clone(), args[arg_index].clone());
                        bound_by_position.insert(param.name.clone());
                        arg_index += 1;
                        continue;
                    }
                    if let Some(value) = kwargs.get(&param.name) {
                        call_env.set_local(param.name.clone(), value.clone());
                        consumed_kwargs.insert(param.name.clone());
                        continue;
                    }
                    if let Some(default_expr) = &param.default {
                        let default_value =
                            self.eval_expr(default_expr, function.env.clone(), &ExecMode::Normal)?;
                        call_env.set_local(param.name.clone(), default_value);
                        continue;
                    }
                    return Err(format!("missing required argument '{}'", param.name));
                }
                ParamKind::VarArgs => {
                    let rest = if arg_index < args.len() {
                        args[arg_index..].to_vec()
                    } else {
                        Vec::new()
                    };
                    arg_index = args.len();
                    call_env.set_local(param.name.clone(), Value::List(Rc::new(RefCell::new(rest))));
                }
                ParamKind::VarKwargs => {
                    var_kwargs_name = Some(param.name.clone());
                }
            }
        }

        if arg_index < args.len() {
            return Err("too many positional arguments".to_owned());
        }

        let mut extra_kwargs = BTreeMap::<Key, Value>::new();
        for (key, value) in kwargs.iter() {
            if consumed_kwargs.contains(key) {
                continue;
            }
            if bound_by_position.contains(key) {
                return Err(format!("multiple values for argument '{key}'"));
            }
            if positional_param_names.contains(key) {
                continue;
            }
            if var_kwargs_name.is_some() {
                extra_kwargs.insert(Key::Str(key.clone()), value.clone());
            } else {
                return Err(format!("unexpected keyword argument '{key}'"));
            }
        }
        if let Some(name) = var_kwargs_name {
            call_env.set_local(name, Value::Dict(Rc::new(RefCell::new(extra_kwargs))));
        }

        if function.is_generator {
            return Ok(Value::Generator(Rc::new(RefCell::new(Generator {
                function,
                call_env,
                computed: false,
                values: Vec::new(),
                index: 0,
            }))));
        }

        self.call_stack.push(call_env.clone());
        let exec_result = self.exec_block(&function.body, call_env, &ExecMode::Normal);
        let _ = self.call_stack.pop();
        let flow = exec_result?;
        match flow {
            Flow::Return(value) => Ok(value),
            Flow::Next | Flow::Break | Flow::Continue => Ok(Value::None),
        }
    }

    fn call_class(
        &mut self,
        class: Rc<Class>,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        let instance = Rc::new(RefCell::new(Instance {
            class: class.clone(),
            fields: HashMap::new(),
        }));
        if class
            .attrs
            .borrow()
            .get("__dataclass__")
            .is_some_and(|v| matches!(v, Value::Bool(true)))
        {
            self.init_dataclass_instance(instance.clone(), class.clone(), args, kwargs)?;
            return Ok(Value::Instance(instance));
        }

        if let Some(init_value) = lookup_class_attr(&class, "__init__").map(|(v, _)| v) {
            let init = match init_value {
                Value::Function(function) => {
                    let owner = lookup_class_attr(&class, "__init__")
                        .map(|(_, owner)| owner)
                        .unwrap_or_else(|| class.clone());
                    Value::BoundMethod(Rc::new(BoundMethod {
                        instance: instance.clone(),
                        function,
                        owner_class: owner,
                    }))
                }
                Value::Builtin(Builtin::Native(f)) => Value::Builtin(Builtin::Native(f)),
                other => other,
            };
            let _ = self.call_callable(init, args, kwargs)?;
        }
        Ok(Value::Instance(instance))
    }

    fn init_dataclass_instance(
        &mut self,
        instance: Rc<RefCell<Instance>>,
        class: Rc<Class>,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
    ) -> RtResult<()> {
        let mut fields = HashMap::<String, Value>::new();
        let mut pos = 0usize;

        for field in &class.field_defs {
            let value = if pos < args.len() {
                let v = args[pos].clone();
                pos += 1;
                v
            } else if let Some(v) = kwargs.get(&field.name) {
                v.clone()
            } else if field.has_default {
                match field.default.clone().unwrap_or(Value::None) {
                    Value::FieldSpec(spec) => {
                        if let Some(factory) = &spec.default_factory {
                            self.call_callable(factory.clone(), Vec::new(), HashMap::new())?
                        } else if spec.has_default {
                            spec.default.clone().unwrap_or(Value::None)
                        } else {
                            Value::None
                        }
                    }
                    v => v,
                }
            } else {
                return Err(format!("missing required field '{}'", field.name));
            };
            fields.insert(field.name.clone(), value);
        }

        if pos < args.len() {
            return Err("too many positional arguments".to_owned());
        }

        for key in kwargs.keys() {
            if !class.field_defs.iter().any(|f| &f.name == key) {
                return Err(format!("unexpected keyword argument '{key}'"));
            }
        }

        instance.borrow_mut().fields = fields;
        Ok(())
    }

    fn call_file_method(
        &mut self,
        file: Rc<RefCell<FileHandle>>,
        method: FileMethod,
        args: Vec<Value>,
        _kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        match method {
            FileMethod::Read => {
                let mut handle = file.borrow_mut();
                let mut data = String::new();
                let _ = handle.file.seek(SeekFrom::Start(0));
                handle
                    .file
                    .read_to_string(&mut data)
                    .map_err(|err| format!("read failed: {err}"))?;
                Ok(Value::Str(data))
            }
            FileMethod::Write => {
                if args.len() != 1 {
                    return Err("write() expects 1 argument".to_owned());
                }
                let text = value_to_string(&args[0]);
                let mut handle = file.borrow_mut();
                handle
                    .file
                    .write_all(text.as_bytes())
                    .map_err(|err| format!("write failed: {err}"))?;
                Ok(Value::Int(text.len() as i64))
            }
            FileMethod::Close => {
                let mut handle = file.borrow_mut();
                handle
                    .file
                    .flush()
                    .map_err(|err| format!("close failed: {err}"))?;
                Ok(Value::None)
            }
            FileMethod::Enter => Ok(Value::File(file)),
            FileMethod::Exit => Ok(Value::None),
        }
    }

    fn call_list_method(
        &mut self,
        list: Rc<RefCell<Vec<Value>>>,
        method: ListMethod,
        args: Vec<Value>,
        _kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        match method {
            ListMethod::Append => {
                if args.len() != 1 {
                    return Err("append() expects one argument".to_owned());
                }
                list.borrow_mut().push(args[0].clone());
                Ok(Value::None)
            }
            ListMethod::Extend => {
                if args.len() != 1 {
                    return Err("extend() expects one argument".to_owned());
                }
                let values = self.collect_iterable(args[0].clone())?;
                list.borrow_mut().extend(values);
                Ok(Value::None)
            }
            ListMethod::Pop => {
                let idx = if args.is_empty() {
                    (list.borrow().len() as i64) - 1
                } else if args.len() == 1 {
                    expect_int(args[0].clone())?
                } else {
                    return Err("pop() expects at most one argument".to_owned());
                };
                let pos = normalize_index(idx, list.borrow().len())?;
                Ok(list.borrow_mut().remove(pos))
            }
            ListMethod::Remove => {
                if args.len() != 1 {
                    return Err("remove() expects one argument".to_owned());
                }
                let needle = &args[0];
                let pos = list.borrow().iter().position(|v| value_eq(v, needle));
                if let Some(idx) = pos {
                    list.borrow_mut().remove(idx);
                    Ok(Value::None)
                } else {
                    Err("list.remove(x): x not in list".to_owned())
                }
            }
            ListMethod::Sort => {
                if !args.is_empty() {
                    return Err("sort() expects no positional arguments".to_owned());
                }
                let reverse = _kwargs
                    .get("reverse")
                    .is_some_and(|v| matches!(v, Value::Bool(true)));
                list.borrow_mut()
                    .sort_by(|a, b| value_to_string(a).cmp(&value_to_string(b)));
                if reverse {
                    list.borrow_mut().reverse();
                }
                Ok(Value::None)
            }
            ListMethod::Copy => Ok(Value::List(Rc::new(RefCell::new(list.borrow().clone())))),
        }
    }

    fn call_dict_method(
        &mut self,
        dict: Rc<RefCell<BTreeMap<Key, Value>>>,
        method: DictMethod,
        args: Vec<Value>,
        _kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        match method {
            DictMethod::Get => {
                if args.is_empty() || args.len() > 2 {
                    return Err("dict.get() expects one or two arguments".to_owned());
                }
                let key = key_from_value(&args[0])?;
                if let Some(v) = dict.borrow().get(&key).cloned() {
                    Ok(v)
                } else if let Some(default) = args.get(1) {
                    Ok(default.clone())
                } else {
                    Ok(Value::None)
                }
            }
            DictMethod::Keys => Ok(Value::List(Rc::new(RefCell::new(
                dict.borrow().keys().cloned().map(Value::from).collect(),
            )))),
            DictMethod::Values => Ok(Value::List(Rc::new(RefCell::new(
                dict.borrow().values().cloned().collect(),
            )))),
            DictMethod::Items => {
                let items = dict
                    .borrow()
                    .iter()
                    .map(|(k, v)| Value::List(Rc::new(RefCell::new(vec![Value::from(k.clone()), v.clone()]))))
                    .collect::<Vec<_>>();
                Ok(Value::List(Rc::new(RefCell::new(items))))
            }
            DictMethod::SetDefault => {
                if args.is_empty() || args.len() > 2 {
                    return Err("dict.setdefault() expects one or two arguments".to_owned());
                }
                let key = key_from_value(&args[0])?;
                if let Some(v) = dict.borrow().get(&key).cloned() {
                    return Ok(v);
                }
                let value = args.get(1).cloned().unwrap_or(Value::None);
                dict.borrow_mut().insert(key, value.clone());
                Ok(value)
            }
            DictMethod::Copy => Ok(Value::Dict(Rc::new(RefCell::new(dict.borrow().clone())))),
        }
    }

    fn call_set_method(
        &mut self,
        set: Rc<RefCell<BTreeSet<Key>>>,
        method: SetMethod,
        args: Vec<Value>,
        _kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        match method {
            SetMethod::Add => {
                if args.len() != 1 {
                    return Err("set.add() expects one argument".to_owned());
                }
                set.borrow_mut().insert(key_from_value(&args[0])?);
                Ok(Value::None)
            }
            SetMethod::Update => {
                if args.len() != 1 {
                    return Err("set.update() expects one argument".to_owned());
                }
                for value in self.collect_iterable(args[0].clone())? {
                    set.borrow_mut().insert(key_from_value(&value)?);
                }
                Ok(Value::None)
            }
            SetMethod::Get => {
                if args.is_empty() || args.len() > 2 {
                    return Err("set.get() expects one or two arguments".to_owned());
                }
                let key = key_from_value(&args[0])?;
                if set.borrow().contains(&key) {
                    Ok(Value::from(key))
                } else if let Some(default) = args.get(1) {
                    Ok(default.clone())
                } else {
                    Ok(Value::None)
                }
            }
            SetMethod::Remove => {
                if args.len() != 1 {
                    return Err("set.remove() expects one argument".to_owned());
                }
                let key = key_from_value(&args[0])?;
                if set.borrow_mut().remove(&key) {
                    Ok(Value::None)
                } else {
                    Err("set.remove(x): x not in set".to_owned())
                }
            }
            SetMethod::Copy => Ok(Value::Set(Rc::new(RefCell::new(set.borrow().clone())))),
        }
    }

    fn call_string_method(
        &mut self,
        value: String,
        method: StringMethod,
        args: Vec<Value>,
        _kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        match method {
            StringMethod::StartsWith => {
                if args.len() != 1 {
                    return Err("startswith() expects one argument".to_owned());
                }
                let prefix = expect_str_arg(&args[0], "startswith()")?;
                Ok(Value::Bool(value.starts_with(&prefix)))
            }
            StringMethod::EndsWith => {
                if args.len() != 1 {
                    return Err("endswith() expects one argument".to_owned());
                }
                let suffix = expect_str_arg(&args[0], "endswith()")?;
                Ok(Value::Bool(value.ends_with(&suffix)))
            }
            StringMethod::IsAlpha => {
                if !args.is_empty() {
                    return Err("isalpha() expects no arguments".to_owned());
                }
                let ok = !value.is_empty() && value.chars().all(|c| c.is_alphabetic());
                Ok(Value::Bool(ok))
            }
            StringMethod::IsAlnum => {
                if !args.is_empty() {
                    return Err("isalnum() expects no arguments".to_owned());
                }
                let ok = !value.is_empty() && value.chars().all(|c| c.is_alphanumeric());
                Ok(Value::Bool(ok))
            }
            StringMethod::IsDigit => {
                if !args.is_empty() {
                    return Err("isdigit() expects no arguments".to_owned());
                }
                let ok = !value.is_empty() && value.chars().all(|c| c.is_numeric());
                Ok(Value::Bool(ok))
            }
            StringMethod::IsLower => {
                if !args.is_empty() {
                    return Err("islower() expects no arguments".to_owned());
                }
                let mut has_cased = false;
                let mut ok = true;
                for ch in value.chars() {
                    if ch.is_alphabetic() {
                        has_cased = true;
                        if !ch.is_lowercase() {
                            ok = false;
                            break;
                        }
                    }
                }
                Ok(Value::Bool(has_cased && ok))
            }
            StringMethod::IsUpper => {
                if !args.is_empty() {
                    return Err("isupper() expects no arguments".to_owned());
                }
                let mut has_cased = false;
                let mut ok = true;
                for ch in value.chars() {
                    if ch.is_alphabetic() {
                        has_cased = true;
                        if !ch.is_uppercase() {
                            ok = false;
                            break;
                        }
                    }
                }
                Ok(Value::Bool(has_cased && ok))
            }
            StringMethod::IsSpace => {
                if !args.is_empty() {
                    return Err("isspace() expects no arguments".to_owned());
                }
                let ok = !value.is_empty() && value.chars().all(|c| c.is_whitespace());
                Ok(Value::Bool(ok))
            }
            StringMethod::Split => {
                if args.len() > 2 {
                    return Err("split() supports at most two arguments".to_owned());
                }
                let maxsplit = if let Some(v) = args.get(1) {
                    Some(expect_int(v.clone())?)
                } else {
                    None
                };
                let parts = if let Some(sep_value) = args.first() {
                    if matches!(sep_value, Value::None) {
                        split_whitespace_limited(&value, maxsplit)
                    } else {
                        let sep = expect_str_arg(sep_value, "split()")?;
                        split_limited(&value, &sep, maxsplit)
                    }
                } else {
                    split_whitespace_limited(&value, maxsplit)
                };
                Ok(Value::List(Rc::new(RefCell::new(parts))))
            }
            StringMethod::Strip => {
                if args.is_empty() {
                    Ok(Value::Str(value.trim().to_owned()))
                } else if args.len() == 1 {
                    let chars = expect_str_arg(&args[0], "strip()")?;
                    Ok(Value::Str(value.trim_matches(|c| chars.contains(c)).to_owned()))
                } else {
                    Err("strip() expects at most one argument".to_owned())
                }
            }
            StringMethod::LStrip => {
                if args.is_empty() {
                    Ok(Value::Str(value.trim_start().to_owned()))
                } else if args.len() == 1 {
                    let chars = expect_str_arg(&args[0], "lstrip()")?;
                    Ok(Value::Str(
                        value.trim_start_matches(|c| chars.contains(c)).to_owned(),
                    ))
                } else {
                    Err("lstrip() expects at most one argument".to_owned())
                }
            }
            StringMethod::RStrip => {
                if args.is_empty() {
                    Ok(Value::Str(value.trim_end().to_owned()))
                } else if args.len() == 1 {
                    let chars = expect_str_arg(&args[0], "rstrip()")?;
                    Ok(Value::Str(
                        value.trim_end_matches(|c| chars.contains(c)).to_owned(),
                    ))
                } else {
                    Err("rstrip() expects at most one argument".to_owned())
                }
            }
            StringMethod::Lower => Ok(Value::Str(value.to_lowercase())),
            StringMethod::Upper => Ok(Value::Str(value.to_uppercase())),
            StringMethod::Capitalize => {
                let mut chars = value.chars();
                if let Some(first) = chars.next() {
                    let mut out = String::new();
                    out.extend(first.to_uppercase());
                    out.push_str(&chars.as_str().to_lowercase());
                    Ok(Value::Str(out))
                } else {
                    Ok(Value::Str(String::new()))
                }
            }
            StringMethod::Replace => {
                if args.len() < 2 || args.len() > 3 {
                    return Err("replace() expects two or three arguments".to_owned());
                }
                let old = expect_str_arg(&args[0], "replace()")?;
                let new = expect_str_arg(&args[1], "replace()")?;
                if let Some(count_value) = args.get(2) {
                    let count = expect_int(count_value.clone())?;
                    if count <= 0 {
                        return Ok(Value::Str(value));
                    }
                    let mut out = String::new();
                    let mut remaining = value.as_str();
                    let mut left = count;
                    while left > 0 {
                        if let Some(pos) = remaining.find(&old) {
                            out.push_str(&remaining[..pos]);
                            out.push_str(&new);
                            remaining = &remaining[pos + old.len()..];
                            left -= 1;
                        } else {
                            break;
                        }
                    }
                    out.push_str(remaining);
                    Ok(Value::Str(out))
                } else {
                    Ok(Value::Str(value.replace(&old, &new)))
                }
            }
            StringMethod::Join => {
                if args.len() != 1 {
                    return Err("join() expects one iterable argument".to_owned());
                }
                let parts = self.collect_iterable(args[0].clone())?;
                let mut out = Vec::new();
                for part in parts {
                    out.push(value_to_string(&part));
                }
                Ok(Value::Str(out.join(&value)))
            }
            StringMethod::Format => {
                let mut out = value;
                for arg in args {
                    if let Some(pos) = out.find("{}") {
                        let replacement = value_to_string(&arg);
                        out = format!("{}{}{}", &out[..pos], replacement, &out[pos + 2..]);
                    }
                }
                Ok(Value::Str(out))
            }
            StringMethod::Encode => Ok(Value::Bytes(Rc::new(RefCell::new(value.into_bytes())))),
        }
    }

    fn call_bytes_method(
        &mut self,
        value: Rc<RefCell<Vec<u8>>>,
        method: BytesMethod,
        _args: Vec<Value>,
        _kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        match method {
            BytesMethod::Decode => {
                let text = String::from_utf8(value.borrow().clone())
                    .map_err(|_| "decode() failed".to_owned())?;
                Ok(Value::Str(text))
            }
        }
    }

    fn call_path_method(
        &mut self,
        value: PathBuf,
        method: PathMethod,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        match method {
            PathMethod::Resolve => {
                let resolved = std::fs::canonicalize(&value).unwrap_or(value);
                Ok(Value::Path(resolved))
            }
            PathMethod::Exists => Ok(Value::Bool(value.exists())),
            PathMethod::IsFile => Ok(Value::Bool(value.is_file())),
            PathMethod::IsDir => Ok(Value::Bool(value.is_dir())),
            PathMethod::ReadText => {
                let _ = kwargs;
                let text = std::fs::read_to_string(&value)
                    .map_err(|err| format!("read_text failed: {err}"))?;
                Ok(Value::Str(text))
            }
            PathMethod::WriteText => {
                if args.is_empty() {
                    return Err("write_text() expects content".to_owned());
                }
                let text = value_to_string(&args[0]);
                std::fs::write(&value, text.as_bytes())
                    .map_err(|err| format!("write_text failed: {err}"))?;
                Ok(Value::Int(text.len() as i64))
            }
            PathMethod::Open => {
                let mut call_args = vec![Value::Str(value.to_string_lossy().to_string())];
                call_args.extend(args);
                builtin_open(self, call_args, kwargs)
            }
            PathMethod::Mkdir => {
                let parents = kwargs
                    .get("parents")
                    .is_some_and(|v| matches!(v, Value::Bool(true)));
                let exist_ok = kwargs
                    .get("exist_ok")
                    .is_some_and(|v| matches!(v, Value::Bool(true)));
                let result = if parents {
                    std::fs::create_dir_all(&value)
                } else {
                    std::fs::create_dir(&value)
                };
                if let Err(err) = result {
                    if !(exist_ok && value.is_dir()) {
                        return Err(format!("mkdir failed: {err}"));
                    }
                }
                Ok(Value::None)
            }
            PathMethod::RelativeTo => {
                if args.len() != 1 {
                    return Err("relative_to() expects one argument".to_owned());
                }
                let base = path_from_value(&args[0])?;
                let rel = value
                    .strip_prefix(&base)
                    .map_err(|_| "relative_to() failed".to_owned())?;
                Ok(Value::Path(rel.to_path_buf()))
            }
        }
    }

    fn call_namespace_method(
        &mut self,
        _value: Rc<RefCell<HashMap<String, Value>>>,
        _method: NamespaceMethod,
        _args: Vec<Value>,
        _kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        Ok(Value::None)
    }

    fn call_arg_parser_method(
        &mut self,
        value: Rc<RefCell<ArgParser>>,
        method: ArgParserMethod,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
    ) -> RtResult<Value> {
        match method {
            ArgParserMethod::AddArgument => {
                if args.is_empty() {
                    return Err("add_argument expects at least one name".to_owned());
                }
                let mut names = Vec::new();
                for arg in &args {
                    names.push(expect_str_arg(arg, "add_argument")?);
                }
                let positional = !names[0].starts_with('-');
                let dest = if let Some(v) = kwargs.get("dest") {
                    expect_str_arg(v, "dest")?
                } else if positional {
                    names[0].clone()
                } else {
                    names
                        .iter()
                        .find(|n| n.starts_with("--"))
                        .map(|n| n.trim_start_matches('-').replace('-', "_"))
                        .unwrap_or_else(|| names[0].trim_start_matches('-').replace('-', "_"))
                };
                let action = if let Some(v) = kwargs.get("action") {
                    match expect_str_arg(v, "action")?.as_str() {
                        "store_true" => ArgAction::StoreTrue,
                        "append" => ArgAction::Append,
                        _ => ArgAction::Store,
                    }
                } else {
                    ArgAction::Store
                };
                let nargs = if kwargs
                    .get("nargs")
                    .is_some_and(|v| matches!(v, Value::Str(s) if s == "*"))
                {
                    ArgNargs::Star
                } else {
                    ArgNargs::One
                };
                let default = if let Some(v) = kwargs.get("default") {
                    v.clone()
                } else {
                    match action {
                        ArgAction::StoreTrue => Value::Bool(false),
                        ArgAction::Append => Value::List(Rc::new(RefCell::new(Vec::new()))),
                        ArgAction::Store => {
                            if positional && nargs == ArgNargs::Star {
                                Value::List(Rc::new(RefCell::new(Vec::new())))
                            } else {
                                Value::None
                            }
                        }
                    }
                };
                let arg_type = kwargs.get("type").cloned();
                value.borrow_mut().specs.push(ArgSpec {
                    names,
                    dest,
                    positional,
                    action,
                    nargs,
                    arg_type,
                    default,
                });
                Ok(Value::None)
            }
            ArgParserMethod::AddMutuallyExclusiveGroup => Ok(Value::ArgParser(value)),
            ArgParserMethod::ParseArgs => {
                let input_args = if args.is_empty() || matches!(args.first(), Some(Value::None)) {
                    self.argv.iter().skip(1).cloned().map(Value::Str).collect::<Vec<_>>()
                } else if args.len() == 1 {
                    self.collect_iterable(args[0].clone())?
                } else {
                    return Err("parse_args expects zero or one argument".to_owned());
                };

                let mut argv = Vec::<String>::new();
                for v in input_args {
                    argv.push(expect_str_arg(&v, "parse_args")?);
                }

                let specs = value.borrow().specs.clone();
                let mut ns = HashMap::<String, Value>::new();
                for spec in &specs {
                    ns.insert(spec.dest.clone(), spec.default.clone());
                }

                let mut positional_values = Vec::<String>::new();
                let mut i = 0usize;
                while i < argv.len() {
                    let token = &argv[i];
                    if token.starts_with('-') {
                        let (opt_name, inline_value) = if let Some((k, v)) = token.split_once('=') {
                            (k.to_owned(), Some(v.to_owned()))
                        } else {
                            (token.clone(), None)
                        };
                        let mut matched = None::<ArgSpec>;
                        for spec in &specs {
                            if !spec.positional && spec.names.iter().any(|n| n == &opt_name) {
                                matched = Some(spec.clone());
                                break;
                            }
                        }
                        let Some(spec) = matched else {
                            i += 1;
                            continue;
                        };
                        match spec.action {
                            ArgAction::StoreTrue => {
                                ns.insert(spec.dest.clone(), Value::Bool(true));
                                i += 1;
                            }
                            ArgAction::Store | ArgAction::Append => {
                                let raw_value = if let Some(v) = inline_value {
                                    v
                                } else {
                                    if i + 1 >= argv.len() {
                                        return Err(format!("option '{token}' requires value"));
                                    }
                                    argv[i + 1].clone()
                                };
                                let mut parsed = Value::Str(raw_value);
                                if let Some(arg_type) = &spec.arg_type {
                                    parsed =
                                        self.call_callable(arg_type.clone(), vec![parsed], HashMap::new())?;
                                }
                                if spec.action == ArgAction::Append {
                                    let entry = ns
                                        .entry(spec.dest.clone())
                                        .or_insert_with(|| Value::List(Rc::new(RefCell::new(Vec::new()))));
                                    if let Value::List(list) = entry {
                                        list.borrow_mut().push(parsed);
                                    }
                                } else {
                                    ns.insert(spec.dest.clone(), parsed);
                                }
                                if token.contains('=') {
                                    i += 1;
                                } else {
                                    i += 2;
                                }
                            }
                        }
                    } else {
                        positional_values.push(token.clone());
                        i += 1;
                    }
                }

                let positionals = specs
                    .iter()
                    .filter(|s| s.positional)
                    .cloned()
                    .collect::<Vec<_>>();
                if !positionals.is_empty() {
                    if positionals.len() == 1 && positionals[0].nargs == ArgNargs::Star {
                        let spec = &positionals[0];
                        let mut out = Vec::new();
                        for item in positional_values {
                            let mut parsed = Value::Str(item);
                            if let Some(arg_type) = &spec.arg_type {
                                parsed =
                                    self.call_callable(arg_type.clone(), vec![parsed], HashMap::new())?;
                            }
                            out.push(parsed);
                        }
                        ns.insert(spec.dest.clone(), Value::List(Rc::new(RefCell::new(out))));
                    } else {
                        for (idx, spec) in positionals.iter().enumerate() {
                            if idx >= positional_values.len() {
                                break;
                            }
                            let mut parsed = Value::Str(positional_values[idx].clone());
                            if let Some(arg_type) = &spec.arg_type {
                                parsed =
                                    self.call_callable(arg_type.clone(), vec![parsed], HashMap::new())?;
                            }
                            ns.insert(spec.dest.clone(), parsed);
                        }
                    }
                }

                Ok(Value::Namespace(Rc::new(RefCell::new(ns))))
            }
        }
    }

    fn get_attr(&mut self, value: &Value, name: &str) -> RtResult<Option<Value>> {
        match value {
            Value::Module(module) => Ok(module.borrow().attrs.get(name).cloned()),
            Value::Class(class) => {
                if name == "__name__" {
                    return Ok(Some(Value::Str(class.name.clone())));
                }
                if let Some((v, _owner)) = lookup_class_attr(class, name) {
                    match v {
                        Value::ClassMethod(function) => {
                            return Ok(Some(Value::BoundClassMethod(Rc::new(BoundClassMethod {
                                class: class.clone(),
                                function,
                            }))));
                        }
                        Value::StaticMethod(inner) => return Ok(Some(*inner)),
                        _ => return Ok(Some(v)),
                    }
                }
                Ok(None)
            }
            Value::Instance(instance) => {
                if name == "__class__" {
                    return Ok(Some(Value::Class(instance.borrow().class.clone())));
                }
                if let Some(v) = instance.borrow().fields.get(name).cloned() {
                    return Ok(Some(v));
                }
                if let Some((v, owner)) = lookup_class_attr(&instance.borrow().class, name) {
                    if let Value::Function(function) = v {
                        return Ok(Some(Value::BoundMethod(Rc::new(BoundMethod {
                            instance: instance.clone(),
                            function,
                            owner_class: owner,
                        }))));
                    }
                    if let Value::ClassMethod(function) = v {
                        return Ok(Some(Value::BoundClassMethod(Rc::new(BoundClassMethod {
                            class: instance.borrow().class.clone(),
                            function,
                        }))));
                    }
                    if let Value::StaticMethod(inner) = v {
                        return Ok(Some(*inner));
                    }
                    if let Value::Property(function) = v {
                        let result = self.call_function(
                            function,
                            Some(instance.clone()),
                            Some(owner),
                            Vec::new(),
                            HashMap::new(),
                        )?;
                        return Ok(Some(result));
                    }
                    return Ok(Some(v));
                }
                Ok(None)
            }
            Value::File(file) => {
                let method = match name {
                    "read" => Some(FileMethod::Read),
                    "write" => Some(FileMethod::Write),
                    "close" => Some(FileMethod::Close),
                    "__enter__" => Some(FileMethod::Enter),
                    "__exit__" => Some(FileMethod::Exit),
                    _ => None,
                };
                Ok(method.map(|m| {
                    Value::Builtin(Builtin::BoundFile {
                        file: file.clone(),
                        method: m,
                    })
                }))
            }
            Value::List(list) => {
                let method = match name {
                    "append" => Some(ListMethod::Append),
                    "extend" => Some(ListMethod::Extend),
                    "pop" => Some(ListMethod::Pop),
                    "remove" => Some(ListMethod::Remove),
                    "sort" => Some(ListMethod::Sort),
                    "copy" => Some(ListMethod::Copy),
                    _ => None,
                };
                Ok(method.map(|m| {
                    Value::Builtin(Builtin::BoundList {
                        list: list.clone(),
                        method: m,
                    })
                }))
            }
            Value::Dict(dict) => {
                let method = match name {
                    "get" => Some(DictMethod::Get),
                    "keys" => Some(DictMethod::Keys),
                    "values" => Some(DictMethod::Values),
                    "items" => Some(DictMethod::Items),
                    "setdefault" => Some(DictMethod::SetDefault),
                    "copy" => Some(DictMethod::Copy),
                    _ => None,
                };
                Ok(method.map(|m| {
                    Value::Builtin(Builtin::BoundDict {
                        dict: dict.clone(),
                        method: m,
                    })
                }))
            }
            Value::Set(set) => {
                let method = match name {
                    "add" => Some(SetMethod::Add),
                    "update" => Some(SetMethod::Update),
                    "get" => Some(SetMethod::Get),
                    "remove" => Some(SetMethod::Remove),
                    "copy" => Some(SetMethod::Copy),
                    _ => None,
                };
                Ok(method.map(|m| {
                    Value::Builtin(Builtin::BoundSet {
                        set: set.clone(),
                        method: m,
                    })
                }))
            }
            Value::Str(text) => {
                let method = match name {
                    "startswith" => Some(StringMethod::StartsWith),
                    "endswith" => Some(StringMethod::EndsWith),
                    "isalpha" => Some(StringMethod::IsAlpha),
                    "isalnum" => Some(StringMethod::IsAlnum),
                    "isdigit" => Some(StringMethod::IsDigit),
                    "islower" => Some(StringMethod::IsLower),
                    "isupper" => Some(StringMethod::IsUpper),
                    "isspace" => Some(StringMethod::IsSpace),
                    "split" => Some(StringMethod::Split),
                    "strip" => Some(StringMethod::Strip),
                    "lstrip" => Some(StringMethod::LStrip),
                    "rstrip" => Some(StringMethod::RStrip),
                    "lower" => Some(StringMethod::Lower),
                    "upper" => Some(StringMethod::Upper),
                    "capitalize" => Some(StringMethod::Capitalize),
                    "replace" => Some(StringMethod::Replace),
                    "join" => Some(StringMethod::Join),
                    "format" => Some(StringMethod::Format),
                    "encode" => Some(StringMethod::Encode),
                    _ => None,
                };
                Ok(method.map(|m| {
                    Value::Builtin(Builtin::BoundString {
                        value: text.clone(),
                        method: m,
                    })
                }))
            }
            Value::Bytes(bytes) => {
                let method = match name {
                    "decode" => Some(BytesMethod::Decode),
                    _ => None,
                };
                Ok(method.map(|m| {
                    Value::Builtin(Builtin::BoundBytes {
                        value: bytes.clone(),
                        method: m,
                    })
                }))
            }
            Value::Path(path) => match name {
                "parent" => Ok(Some(Value::Path(
                    path.parent().unwrap_or_else(|| Path::new("")).to_path_buf(),
                ))),
                "parents" => {
                    let mut out = Vec::new();
                    let mut cur = path.parent();
                    while let Some(p) = cur {
                        out.push(Value::Path(p.to_path_buf()));
                        cur = p.parent();
                    }
                    Ok(Some(Value::List(Rc::new(RefCell::new(out)))))
                }
                "name" => Ok(Some(Value::Str(
                    path.file_name()
                        .map(|v| v.to_string_lossy().to_string())
                        .unwrap_or_default(),
                ))),
                "stem" => Ok(Some(Value::Str(
                    path.file_stem()
                        .map(|v| v.to_string_lossy().to_string())
                        .unwrap_or_default(),
                ))),
                "suffix" => Ok(Some(Value::Str(
                    path.extension()
                        .map(|v| format!(".{}", v.to_string_lossy()))
                        .unwrap_or_default(),
                ))),
                _ => {
                    let method = match name {
                        "resolve" => Some(PathMethod::Resolve),
                        "exists" => Some(PathMethod::Exists),
                        "is_file" => Some(PathMethod::IsFile),
                        "is_dir" => Some(PathMethod::IsDir),
                        "read_text" => Some(PathMethod::ReadText),
                        "write_text" => Some(PathMethod::WriteText),
                        "open" => Some(PathMethod::Open),
                        "mkdir" => Some(PathMethod::Mkdir),
                        "relative_to" => Some(PathMethod::RelativeTo),
                        _ => None,
                    };
                    Ok(method.map(|m| {
                        Value::Builtin(Builtin::BoundPath {
                            value: path.clone(),
                            method: m,
                        })
                    }))
                }
            },
            Value::Namespace(ns) => Ok(ns.borrow().get(name).cloned()),
            Value::ArgParser(parser) => {
                let method = match name {
                    "add_argument" => Some(ArgParserMethod::AddArgument),
                    "add_mutually_exclusive_group" => Some(ArgParserMethod::AddMutuallyExclusiveGroup),
                    "parse_args" => Some(ArgParserMethod::ParseArgs),
                    _ => None,
                };
                Ok(method.map(|m| {
                    Value::Builtin(Builtin::BoundArgParser {
                        value: parser.clone(),
                        method: m,
                    })
                }))
            }
            Value::Builtin(Builtin::Native(f)) => {
                if (*f as usize) == (builtin_int as usize) && name == "from_bytes" {
                    return Ok(Some(Value::Builtin(Builtin::Native(int_from_bytes))));
                }
                Ok(None)
            }
            Value::Super(sup) => {
                if let Some((attr, owner)) = lookup_class_attr(&sup.class, name) {
                    if let Value::Function(function) = attr {
                        return Ok(Some(Value::BoundMethod(Rc::new(BoundMethod {
                            instance: sup.instance.clone(),
                            function,
                            owner_class: owner,
                        }))));
                    }
                    return Ok(Some(attr));
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn set_attr(&mut self, value: Value, name: &str, attr_value: Value) -> RtResult<()> {
        match value {
            Value::Module(module) => {
                module
                    .borrow_mut()
                    .attrs
                    .insert(name.to_owned(), attr_value);
                Ok(())
            }
            Value::Class(class) => {
                class.attrs.borrow_mut().insert(name.to_owned(), attr_value);
                Ok(())
            }
            Value::Instance(instance) => {
                instance
                    .borrow_mut()
                    .fields
                    .insert(name.to_owned(), attr_value);
                Ok(())
            }
            Value::Namespace(ns) => {
                ns.borrow_mut().insert(name.to_owned(), attr_value);
                Ok(())
            }
            _ => Err("attribute assignment target is not object-like".to_owned()),
        }
    }

    fn get_item(&mut self, value: Value, index: Value) -> RtResult<Value> {
        match value {
            Value::List(items) => {
                if let Value::List(slice_parts) = &index {
                    let parts = slice_parts.borrow();
                    if parts.len() == 2 {
                        let len = items.borrow().len() as i64;
                        let start = normalize_slice_bound(parts.first().cloned().unwrap_or(Value::None), len, 0)?;
                        let stop = normalize_slice_bound(parts.get(1).cloned().unwrap_or(Value::None), len, len)?;
                        let mut out = Vec::new();
                        for i in start..stop {
                            if let Some(v) = items.borrow().get(i as usize).cloned() {
                                out.push(v);
                            }
                        }
                        return Ok(Value::List(Rc::new(RefCell::new(out))));
                    }
                }
                if matches!(index, Value::Str(ref s) if s == ":") {
                    return Ok(Value::List(Rc::new(RefCell::new(items.borrow().clone()))));
                }
                let idx = normalize_index(expect_int(index)?, items.borrow().len())?;
                items
                    .borrow()
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| "list index out of range".to_owned())
            }
            Value::Dict(items) => {
                let key = key_from_value(&index)?;
                items
                    .borrow()
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| "dict key not found".to_owned())
            }
            Value::Str(text) => {
                if let Value::List(slice_parts) = &index {
                    let chars = text.chars().collect::<Vec<_>>();
                    let len = chars.len() as i64;
                    let parts = slice_parts.borrow();
                    if parts.len() == 2 {
                        let start = normalize_slice_bound(parts.first().cloned().unwrap_or(Value::None), len, 0)?;
                        let stop = normalize_slice_bound(parts.get(1).cloned().unwrap_or(Value::None), len, len)?;
                        let mut out = String::new();
                        for i in start..stop {
                            if let Some(ch) = chars.get(i as usize) {
                                out.push(*ch);
                            }
                        }
                        return Ok(Value::Str(out));
                    }
                }
                let idx = normalize_index(expect_int(index)?, text.chars().count())?;
                let ch = text
                    .chars()
                    .nth(idx)
                    .ok_or_else(|| "string index out of range".to_owned())?;
                Ok(Value::Str(ch.to_string()))
            }
            Value::Bytes(bytes) => {
                if let Value::List(slice_parts) = &index {
                    let data = bytes.borrow();
                    let len = data.len() as i64;
                    let parts = slice_parts.borrow();
                    if parts.len() == 2 {
                        let start =
                            normalize_slice_bound(parts.first().cloned().unwrap_or(Value::None), len, 0)?;
                        let stop = normalize_slice_bound(
                            parts.get(1).cloned().unwrap_or(Value::None),
                            len,
                            len,
                        )?;
                        let mut out = Vec::new();
                        for i in start..stop {
                            if let Some(v) = data.get(i as usize) {
                                out.push(*v);
                            }
                        }
                        return Ok(Value::Bytes(Rc::new(RefCell::new(out))));
                    }
                }
                let idx = normalize_index(expect_int(index)?, bytes.borrow().len())?;
                Ok(Value::Int(bytes.borrow()[idx] as i64))
            }
            Value::TypingAlias(name) => Ok(Value::TypingAlias(name.clone())),
            _ => Err("object is not subscriptable".to_owned()),
        }
    }

    fn set_item(&mut self, value: Value, index: Value, rhs: Value) -> RtResult<()> {
        match value {
            Value::List(items) => {
                if let Value::List(slice_parts) = &index {
                    let parts = slice_parts.borrow();
                    if parts.len() == 2 {
                        let len = items.borrow().len() as i64;
                        let start = normalize_slice_bound(
                            parts.first().cloned().unwrap_or(Value::None),
                            len,
                            0,
                        )?;
                        let stop = normalize_slice_bound(
                            parts.get(1).cloned().unwrap_or(Value::None),
                            len,
                            len,
                        )?;
                        let replacement = match rhs {
                            Value::List(values) => values.borrow().clone(),
                            other => vec![other],
                        };
                        let mut borrow = items.borrow_mut();
                        let start_u = start as usize;
                        let stop_u = stop as usize;
                        borrow.splice(start_u..stop_u, replacement);
                        return Ok(());
                    }
                }
                if matches!(index, Value::Str(ref s) if s == ":") {
                    let replacement = match rhs {
                        Value::List(values) => values.borrow().clone(),
                        other => vec![other],
                    };
                    *items.borrow_mut() = replacement;
                    return Ok(());
                }
                let idx = normalize_index(expect_int(index)?, items.borrow().len())?;
                let mut borrow = items.borrow_mut();
                if idx >= borrow.len() {
                    return Err("list assignment index out of range".to_owned());
                }
                borrow[idx] = rhs;
                Ok(())
            }
            Value::Dict(items) => {
                let key = key_from_value(&index)?;
                items.borrow_mut().insert(key, rhs);
                Ok(())
            }
            _ => Err("object does not support indexed assignment".to_owned()),
        }
    }

    fn collect_iterable(&mut self, value: Value) -> RtResult<Vec<Value>> {
        match value {
            Value::List(items) => Ok(items.borrow().clone()),
            Value::Set(items) => Ok(items.borrow().iter().cloned().map(Value::from).collect()),
            Value::Dict(items) => Ok(items.borrow().keys().cloned().map(Value::from).collect()),
            Value::Str(text) => Ok(text.chars().map(|c| Value::Str(c.to_string())).collect()),
            Value::Bytes(bytes) => Ok(bytes
                .borrow()
                .iter()
                .map(|v| Value::Int(*v as i64))
                .collect()),
            Value::Range(range) => {
                if range.step == 0 {
                    return Err("range step cannot be zero".to_owned());
                }
                let mut out = Vec::new();
                let mut current = range.start;
                if range.step > 0 {
                    while current < range.stop {
                        out.push(Value::Int(current));
                        current += range.step;
                    }
                } else {
                    while current > range.stop {
                        out.push(Value::Int(current));
                        current += range.step;
                    }
                }
                Ok(out)
            }
            Value::Generator(generator) => {
                let mut out = Vec::new();
                while let Some(value) = self.generator_next(&generator)? {
                    out.push(value);
                }
                Ok(out)
            }
            Value::File(file) => {
                let mut handle = file.borrow_mut();
                let mut data = String::new();
                let _ = handle.file.seek(SeekFrom::Start(0));
                handle
                    .file
                    .read_to_string(&mut data)
                    .map_err(|err| format!("read failed: {err}"))?;
                Ok(data.lines().map(|line| Value::Str(line.to_owned())).collect())
            }
            Value::Path(path) => Ok(path
                .to_string_lossy()
                .chars()
                .map(|c| Value::Str(c.to_string()))
                .collect()),
            _ => Err(format!(
                "object '{}' is not iterable",
                display_value(&value)
            )),
        }
    }

    fn generator_next(&mut self, generator: &Rc<RefCell<Generator>>) -> RtResult<Option<Value>> {
        let mut gen = generator.borrow_mut();
        if !gen.computed {
            let yielded = Rc::new(RefCell::new(Vec::<Value>::new()));
            let mode = ExecMode::CollectYield(yielded.clone());
            let _ = self.exec_block(&gen.function.body, gen.call_env.clone(), &mode)?;
            gen.values = yielded.borrow().clone();
            gen.computed = true;
        }
        if gen.index >= gen.values.len() {
            Ok(None)
        } else {
            let value = gen.values[gen.index].clone();
            gen.index += 1;
            Ok(Some(value))
        }
    }

    fn is_truthy(&self, value: &Value) -> bool {
        match value {
            Value::None => false,
            Value::Bool(v) => *v,
            Value::Int(v) => *v != 0,
            Value::Str(v) => !v.is_empty(),
            Value::Bytes(v) => !v.borrow().is_empty(),
            Value::List(v) => !v.borrow().is_empty(),
            Value::Dict(v) => !v.borrow().is_empty(),
            Value::Set(v) => !v.borrow().is_empty(),
            Value::Namespace(v) => !v.borrow().is_empty(),
            _ => true,
        }
    }

    fn import_module(&mut self, name: &str) -> RtResult<Value> {
        if let Some(value) = self.modules.get(name).cloned() {
            if let Some((parent, leaf)) = name.rsplit_once('.') {
                if let Some(Value::Module(parent_mod)) = self.modules.get(parent).cloned() {
                    parent_mod
                        .borrow_mut()
                        .attrs
                        .insert(leaf.to_owned(), value.clone());
                }
            }
            return Ok(value);
        }

        let mut parent_name = None::<String>;
        let mut leaf_name = None::<String>;
        if let Some((parent, leaf)) = name.rsplit_once('.') {
            parent_name = Some(parent.to_owned());
            leaf_name = Some(leaf.to_owned());
            let _ = self.import_module(parent)?;
        }

        // Parent package initialization may import this module as a side effect.
        if let Some(value) = self.modules.get(name).cloned() {
            if let (Some(parent), Some(leaf)) = (parent_name.clone(), leaf_name.clone()) {
                if let Some(Value::Module(parent_mod)) = self.modules.get(&parent).cloned() {
                    parent_mod.borrow_mut().attrs.insert(leaf, value.clone());
                }
            }
            return Ok(value);
        }

        let root = name.split('.').next().unwrap_or(name);
        if self.blocked_roots.contains(root) {
            return Err(format!("nanopy does not support module '{root}'"));
        }

        if let Some(module) = self.try_builtin_module(name)? {
            self.modules.insert(name.to_owned(), module.clone());
            return Ok(module);
        }

        let path = resolve_module_path(name, &self.search_paths)
            .ok_or_else(|| format!("unable to locate module '{name}'"))?;
        let source = std::fs::read_to_string(&path)
            .map_err(|err| format!("failed reading module '{}': {err}", path.display()))?;

        let module_rc = Rc::new(RefCell::new(Module {
            name: name.to_owned(),
            attrs: HashMap::new(),
        }));
        let module = Value::Module(module_rc.clone());
        self.modules.insert(name.to_owned(), module.clone());

        let tokens =
            lexer::lex(&source).map_err(|err| format!("{}: {err}", path.to_string_lossy()))?;
        let program =
            parser::parse(tokens).map_err(|err| format!("{}: {err}", path.to_string_lossy()))?;
        let module_env = Env::new(None);
        self.install_builtins(&module_env);
        module_env.set_local("__name__", Value::Str(name.to_owned()));
        module_env.set_local("__file__", Value::Str(path.to_string_lossy().to_string()));
        self.exec_block(&program.body, module_env.clone(), &ExecMode::Normal)?;
        module_rc.borrow_mut().attrs = module_env.values_snapshot();

        if let (Some(parent), Some(leaf)) = (parent_name, leaf_name) {
            if let Some(Value::Module(parent_mod)) = self.modules.get(&parent).cloned() {
                parent_mod.borrow_mut().attrs.insert(leaf, module.clone());
            }
        }
        Ok(module)
    }

    fn try_builtin_module(&mut self, name: &str) -> RtResult<Option<Value>> {
        match name {
            "os" => Ok(Some(make_os_module())),
            "os.path" => {
                if let Value::Module(module) = make_os_module() {
                    return Ok(module.borrow().attrs.get("path").cloned());
                }
                Ok(None)
            }
            "sys" => Ok(Some(self.make_sys_module())),
            "gc" => Ok(Some(make_gc_module())),
            "dataclasses" => Ok(Some(make_dataclasses_module())),
            "enum" => Ok(Some(make_enum_module())),
            "argparse" => Ok(Some(make_argparse_module())),
            "pathlib" => Ok(Some(make_pathlib_module())),
            "typing" => Ok(Some(make_typing_module())),
            "copy" => Ok(Some(make_copy_module())),
            "abc" => Ok(Some(make_abc_module())),
            "warnings" => Ok(Some(make_warnings_module())),
            "keyword" => Ok(Some(make_keyword_module())),
            "__future__" => Ok(Some(make_future_module())),
            _ => Ok(None),
        }
    }

    fn make_sys_module(&self) -> Value {
        let mut attrs = HashMap::new();
        attrs.insert(
            "argv".to_owned(),
            Value::List(Rc::new(RefCell::new(
                self.argv.iter().cloned().map(Value::Str).collect(),
            ))),
        );
        attrs.insert(
            "path".to_owned(),
            Value::List(Rc::new(RefCell::new(
                self.search_paths
                    .iter()
                    .map(|p| Value::Str(p.to_string_lossy().to_string()))
                    .collect(),
            ))),
        );
        attrs.insert(
            "version".to_owned(),
            Value::Str("3.8.0 (nanopy)".to_owned()),
        );
        if let Ok(file) = OpenOptions::new().write(true).open("/dev/stdout") {
            attrs.insert(
                "stdout".to_owned(),
                Value::File(Rc::new(RefCell::new(FileHandle { file }))),
            );
        }
        if let Ok(file) = OpenOptions::new().write(true).open("/dev/stderr") {
            attrs.insert(
                "stderr".to_owned(),
                Value::File(Rc::new(RefCell::new(FileHandle { file }))),
            );
        }
        attrs.insert("exit".to_owned(), Value::Builtin(Builtin::Native(sys_exit)));
        Value::Module(Rc::new(RefCell::new(Module {
            name: "sys".to_owned(),
            attrs,
        })))
    }
}

fn resolve_module_path(module: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let rel = module.replace('.', "/");
    for root in roots {
        let file_path = root.join(format!("{rel}.py"));
        if file_path.is_file() {
            return Some(file_path);
        }
        let pkg_path = root.join(&rel).join("__init__.py");
        if pkg_path.is_file() {
            return Some(pkg_path);
        }
    }
    None
}

fn resolve_module_entry_path(module: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let rel = module.replace('.', "/");
    for root in roots {
        let pkg_main = root.join(&rel).join("__main__.py");
        if pkg_main.is_file() {
            return Some(pkg_main);
        }
        let file_path = root.join(format!("{rel}.py"));
        if file_path.is_file() {
            return Some(file_path);
        }
    }
    None
}

fn lookup_class_attr(class: &Rc<Class>, name: &str) -> Option<(Value, Rc<Class>)> {
    if let Some(v) = class.attrs.borrow().get(name).cloned() {
        return Some((v, class.clone()));
    }
    for base in &class.bases {
        if let Some(v) = lookup_class_attr(base, name) {
            return Some(v);
        }
    }
    None
}

fn is_subclass_of(class: &Rc<Class>, target: &Rc<Class>) -> bool {
    if Rc::ptr_eq(class, target) {
        return true;
    }
    class.bases.iter().any(|base| is_subclass_of(base, target))
}

fn is_instance_of(value: &Value, typ: &Value) -> bool {
    match typ {
        Value::Class(target) => match value {
            Value::Instance(inst) => is_subclass_of(&inst.borrow().class, target),
            Value::Class(cls) => is_subclass_of(cls, target),
            _ => false,
        },
        Value::List(items) => items.borrow().iter().any(|t| is_instance_of(value, t)),
        Value::Builtin(Builtin::Native(f)) if (*f as usize) == (builtin_int as usize) => {
            matches!(value, Value::Int(_) | Value::Bool(_))
        }
        Value::Builtin(Builtin::Native(f)) if (*f as usize) == (builtin_str as usize) => {
            matches!(value, Value::Str(_))
        }
        Value::Builtin(Builtin::Native(f)) if (*f as usize) == (builtin_bool as usize) => {
            matches!(value, Value::Bool(_))
        }
        Value::Builtin(Builtin::Native(f)) if (*f as usize) == (builtin_float as usize) => {
            matches!(value, Value::Int(_))
        }
        Value::Builtin(Builtin::Native(f)) if (*f as usize) == (builtin_list as usize) => {
            matches!(value, Value::List(_))
        }
        Value::Builtin(Builtin::Native(f)) if (*f as usize) == (builtin_dict as usize) => {
            matches!(value, Value::Dict(_))
        }
        Value::Builtin(Builtin::Native(f)) if (*f as usize) == (builtin_set as usize) => {
            matches!(value, Value::Set(_))
        }
        _ => false,
    }
}

fn expect_str_arg(value: &Value, func: &str) -> RtResult<String> {
    match value {
        Value::Str(v) => Ok(v.clone()),
        Value::Path(v) => Ok(v.to_string_lossy().to_string()),
        _ => Err(format!("{func} expects string argument")),
    }
}

fn split_limited(value: &str, sep: &str, maxsplit: Option<i64>) -> Vec<Value> {
    if sep.is_empty() {
        return value.chars().map(|c| Value::Str(c.to_string())).collect();
    }
    let mut out = Vec::new();
    if let Some(limit) = maxsplit {
        if limit <= 0 {
            return vec![Value::Str(value.to_owned())];
        }
        let mut rest = value;
        let mut left = limit;
        while left > 0 {
            if let Some(pos) = rest.find(sep) {
                out.push(Value::Str(rest[..pos].to_owned()));
                rest = &rest[pos + sep.len()..];
                left -= 1;
            } else {
                break;
            }
        }
        out.push(Value::Str(rest.to_owned()));
        return out;
    }
    value
        .split(sep)
        .map(|s| Value::Str(s.to_owned()))
        .collect::<Vec<_>>()
}

fn split_whitespace_limited(value: &str, maxsplit: Option<i64>) -> Vec<Value> {
    if let Some(limit) = maxsplit {
        if limit <= 0 {
            return vec![Value::Str(value.to_owned())];
        }
        let parts = value.split_whitespace().collect::<Vec<_>>();
        if parts.is_empty() {
            return Vec::new();
        }
        if (parts.len() as i64) <= limit + 1 {
            return parts
                .into_iter()
                .map(|s| Value::Str(s.to_owned()))
                .collect::<Vec<_>>();
        }
        let mut out = Vec::new();
        for part in parts.iter().take(limit as usize) {
            out.push(Value::Str((*part).to_owned()));
        }
        let tail = parts
            .iter()
            .skip(limit as usize)
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        out.push(Value::Str(tail));
        return out;
    }
    value
        .split_whitespace()
        .map(|s| Value::Str(s.to_owned()))
        .collect::<Vec<_>>()
}

fn path_from_value(value: &Value) -> RtResult<PathBuf> {
    match value {
        Value::Path(v) => Ok(v.clone()),
        Value::Str(v) => Ok(PathBuf::from(v)),
        _ => Err("expected path-like value".to_owned()),
    }
}

fn deep_copy_value(value: &Value) -> Value {
    match value {
        Value::None => Value::None,
        Value::Bool(v) => Value::Bool(*v),
        Value::Int(v) => Value::Int(*v),
        Value::Str(v) => Value::Str(v.clone()),
        Value::Bytes(v) => Value::Bytes(Rc::new(RefCell::new(v.borrow().clone()))),
        Value::List(v) => Value::List(Rc::new(RefCell::new(
            v.borrow().iter().map(deep_copy_value).collect(),
        ))),
        Value::Dict(v) => Value::Dict(Rc::new(RefCell::new(
            v.borrow()
                .iter()
                .map(|(k, v)| (k.clone(), deep_copy_value(v)))
                .collect(),
        ))),
        Value::Set(v) => Value::Set(Rc::new(RefCell::new(v.borrow().clone()))),
        Value::Function(v) => Value::Function(v.clone()),
        Value::ClassMethod(v) => Value::ClassMethod(v.clone()),
        Value::StaticMethod(v) => Value::StaticMethod(Box::new(deep_copy_value(v))),
        Value::Property(v) => Value::Property(v.clone()),
        Value::Builtin(v) => Value::Builtin(v.clone()),
        Value::Class(v) => Value::Class(v.clone()),
        Value::BoundMethod(v) => Value::BoundMethod(v.clone()),
        Value::BoundClassMethod(v) => Value::BoundClassMethod(v.clone()),
        Value::Instance(v) => {
            let cloned = v.borrow();
            let mut fields = HashMap::new();
            for (k, val) in &cloned.fields {
                fields.insert(k.clone(), deep_copy_value(val));
            }
            Value::Instance(Rc::new(RefCell::new(Instance {
                class: cloned.class.clone(),
                fields,
            })))
        }
        Value::Module(v) => Value::Module(v.clone()),
        Value::File(v) => Value::File(v.clone()),
        Value::Path(v) => Value::Path(v.clone()),
        Value::Namespace(v) => Value::Namespace(Rc::new(RefCell::new(
            v.borrow()
                .iter()
                .map(|(k, val)| (k.clone(), deep_copy_value(val)))
                .collect(),
        ))),
        Value::ArgParser(v) => Value::ArgParser(v.clone()),
        Value::Super(v) => Value::Super(v.clone()),
        Value::FieldSpec(v) => Value::FieldSpec(v.clone()),
        Value::AutoEnum => Value::AutoEnum,
        Value::TypingAlias(v) => Value::TypingAlias(v.clone()),
        Value::Range(v) => Value::Range(v.clone()),
        Value::Generator(v) => Value::Generator(v.clone()),
    }
}

fn finalize_enum_class(class: Rc<Class>, attr_order: &[String]) -> RtResult<()> {
    let is_enum = class
        .bases
        .iter()
        .any(|b| b.is_enum_base || b.name == "Enum" || b.name == "IntEnum");
    if !is_enum {
        return Ok(());
    }
    let mut next_auto = 1i64;
    let mut attrs = class.attrs.borrow_mut();
    for name in attr_order {
        if name.starts_with("__") {
            continue;
        }
        let Some(raw) = attrs.get(name).cloned() else {
            continue;
        };
        if matches!(raw, Value::Function(_) | Value::Builtin(_) | Value::Class(_)) {
            continue;
        }
        let value = match raw {
            Value::AutoEnum => {
                let out = Value::Int(next_auto);
                next_auto += 1;
                out
            }
            other => other,
        };
        let mut fields = HashMap::new();
        fields.insert("name".to_owned(), Value::Str(name.clone()));
        fields.insert("value".to_owned(), value);
        let member = Value::Instance(Rc::new(RefCell::new(Instance {
            class: class.clone(),
            fields,
        })));
        attrs.insert(name.clone(), member);
    }
    Ok(())
}

fn contains_yield(body: &[Stmt]) -> bool {
    body.iter().any(stmt_contains_yield)
}

fn stmt_contains_yield(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(expr) => expr_contains_yield(expr),
        Stmt::Assign { target, value } => expr_contains_yield(target) || expr_contains_yield(value),
        Stmt::AugAssign { target, value, .. } => {
            expr_contains_yield(target) || expr_contains_yield(value)
        }
        Stmt::AnnAssign { target, value } => {
            expr_contains_yield(target) || value.as_ref().is_some_and(expr_contains_yield)
        }
        Stmt::If { test, body, orelse } => {
            expr_contains_yield(test) || contains_yield(body) || contains_yield(orelse)
        }
        Stmt::While { test, body } => expr_contains_yield(test) || contains_yield(body),
        Stmt::For { iter, body, .. } => expr_contains_yield(iter) || contains_yield(body),
        Stmt::FunctionDef { .. } => false,
        Stmt::ClassDef { body, .. } => contains_yield(body),
        Stmt::Return(expr) => expr.as_ref().is_some_and(expr_contains_yield),
        Stmt::Raise(expr) => expr.as_ref().is_some_and(expr_contains_yield),
        Stmt::With { context, body, .. } => expr_contains_yield(context) || contains_yield(body),
        Stmt::Try {
            body,
            handlers,
            orelse,
            finalbody,
        } => {
            contains_yield(body)
                || handlers.iter().any(|h| contains_yield(&h.body))
                || contains_yield(orelse)
                || contains_yield(finalbody)
        }
        Stmt::Break | Stmt::Continue | Stmt::Pass | Stmt::Import(_) | Stmt::FromImport { .. } => {
            false
        }
    }
}

fn expr_contains_yield(expr: &Expr) -> bool {
    match expr {
        Expr::Yield(_) => true,
        Expr::Unary { expr, .. } => expr_contains_yield(expr),
        Expr::Binary { left, right, .. } => expr_contains_yield(left) || expr_contains_yield(right),
        Expr::IfExpr {
            then_expr,
            condition,
            else_expr,
        } => {
            expr_contains_yield(then_expr)
                || expr_contains_yield(condition)
                || expr_contains_yield(else_expr)
        }
        Expr::Lambda { body, .. } => expr_contains_yield(body),
        Expr::Call { func, args, kwargs } => {
            expr_contains_yield(func)
                || args.iter().any(expr_contains_yield)
                || kwargs.iter().any(|(_, v)| expr_contains_yield(v))
        }
        Expr::Attr { value, .. } => expr_contains_yield(value),
        Expr::Subscript { value, index } => {
            expr_contains_yield(value) || expr_contains_yield(index)
        }
        Expr::Slice { start, stop } => {
            start.as_ref().is_some_and(|v| expr_contains_yield(v))
                || stop.as_ref().is_some_and(|v| expr_contains_yield(v))
        }
        Expr::Starred(inner) => expr_contains_yield(inner),
        Expr::List(values) | Expr::Set(values) => values.iter().any(expr_contains_yield),
        Expr::Dict(items) => items
            .iter()
            .any(|(k, v)| expr_contains_yield(k) || expr_contains_yield(v)),
        Expr::ListComp {
            elem,
            target,
            iter,
            cond,
        }
        | Expr::SetComp {
            elem,
            target,
            iter,
            cond,
        }
        | Expr::GenComp {
            elem,
            target,
            iter,
            cond,
        } => {
            expr_contains_yield(elem)
                || expr_contains_yield(target)
                || expr_contains_yield(iter)
                || cond.as_ref().is_some_and(|c| expr_contains_yield(c))
        }
        Expr::DictComp {
            key,
            value,
            target,
            iter,
            cond,
        } => {
            expr_contains_yield(key)
                || expr_contains_yield(value)
                || expr_contains_yield(target)
                || expr_contains_yield(iter)
                || cond.as_ref().is_some_and(|c| expr_contains_yield(c))
        }
        Expr::Name(_) | Expr::Int(_) | Expr::Str(_) | Expr::FStr(_) | Expr::Bool(_) | Expr::None => false,
    }
}

fn eval_compare(left: Value, right: Value, pred: fn(std::cmp::Ordering) -> bool) -> RtResult<Value> {
    let ord = match (left, right) {
        (Value::Int(a), Value::Int(b)) => a.cmp(&b),
        (Value::Str(a), Value::Str(b)) => a.cmp(&b),
        _ => return Err("comparison expects compatible operands".to_owned()),
    };
    Ok(Value::Bool(pred(ord)))
}

fn eval_contains(left: Value, right: Value) -> RtResult<Value> {
    let result = match right {
        Value::List(values) => values.borrow().iter().any(|v| value_eq(v, &left)),
        Value::Set(values) => values
            .borrow()
            .contains(&key_from_value(&left).map_err(|e| e.to_string())?),
        Value::Dict(values) => values
            .borrow()
            .contains_key(&key_from_value(&left).map_err(|e| e.to_string())?),
        Value::Str(text) => match left {
            Value::Str(needle) => text.contains(&needle),
            _ => false,
        },
        Value::Bytes(values) => match left {
            Value::Int(v) => values.borrow().iter().any(|b| *b as i64 == v),
            _ => false,
        },
        other => {
            return Err(format!(
                "'in' expects iterable right operand, got {}",
                display_value(&other)
            ))
        }
    };
    Ok(Value::Bool(result))
}

fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::None, Value::None) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bytes(x), Value::Bytes(y)) => x.borrow().as_slice() == y.borrow().as_slice(),
        (Value::Path(x), Value::Path(y)) => x == y,
        (Value::Class(x), Value::Class(y)) => Rc::ptr_eq(x, y),
        (Value::Instance(x), Value::Instance(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

fn expect_int(value: Value) -> RtResult<i64> {
    match value {
        Value::Int(v) => Ok(v),
        _ => Err("expected int".to_owned()),
    }
}

fn normalize_index(index: i64, len: usize) -> RtResult<usize> {
    let len_i = len as i64;
    let idx = if index < 0 { len_i + index } else { index };
    if idx < 0 || idx >= len_i {
        return Err("index out of range".to_owned());
    }
    Ok(idx as usize)
}

fn normalize_slice_bound(value: Value, len: i64, default: i64) -> RtResult<i64> {
    let raw = match value {
        Value::None => default,
        Value::Int(v) => v,
        _ => return Err("slice index must be int or None".to_owned()),
    };
    let mut out = if raw < 0 { len + raw } else { raw };
    if out < 0 {
        out = 0;
    }
    if out > len {
        out = len;
    }
    Ok(out)
}

fn key_from_value(value: &Value) -> RtResult<Key> {
    match value {
        Value::None => Ok(Key::None),
        Value::Bool(v) => Ok(Key::Bool(*v)),
        Value::Int(v) => Ok(Key::Int(*v)),
        Value::Str(v) => Ok(Key::Str(v.clone())),
        Value::Path(v) => Ok(Key::Str(v.to_string_lossy().to_string())),
        Value::List(items) => {
            let mut out = Vec::new();
            for item in items.borrow().iter() {
                out.push(key_from_value(item)?);
            }
            Ok(Key::Tuple(out))
        }
        Value::Instance(instance) => {
            let inst = instance.borrow();
            if let Some(Value::Str(name)) = inst.fields.get("name") {
                return Ok(Key::Str(format!("{}::{name}", inst.class.name)));
            }
            Err("dict/set keys must be hashable primitive values".to_owned())
        }
        _ => Err("dict/set keys must be hashable primitive values".to_owned()),
    }
}

impl From<Key> for Value {
    fn from(value: Key) -> Self {
        match value {
            Key::None => Value::None,
            Key::Bool(v) => Value::Bool(v),
            Key::Int(v) => Value::Int(v),
            Key::Str(v) => Value::Str(v),
            Key::Tuple(values) => {
                Value::List(Rc::new(RefCell::new(
                    values.into_iter().map(Value::from).collect::<Vec<_>>(),
                )))
            }
        }
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::None => "None".to_owned(),
        Value::Bool(v) => {
            if *v {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Value::Int(v) => v.to_string(),
        Value::Str(v) => v.clone(),
        Value::Bytes(v) => format!("b{:?}", v.borrow()),
        Value::List(items) => {
            let parts = items
                .borrow()
                .iter()
                .map(value_to_string)
                .collect::<Vec<_>>();
            format!("[{}]", parts.join(", "))
        }
        Value::Dict(items) => {
            let parts = items
                .borrow()
                .iter()
                .map(|(k, v)| format!("{}: {}", key_to_string(k), value_to_string(v)))
                .collect::<Vec<_>>();
            format!("{{{}}}", parts.join(", "))
        }
        Value::Set(items) => {
            let parts = items.borrow().iter().map(key_to_string).collect::<Vec<_>>();
            if parts.is_empty() {
                "set()".to_owned()
            } else {
                format!("{{{}}}", parts.join(", "))
            }
        }
        Value::Function(function) => format!("<function {}>", function.name),
        Value::ClassMethod(function) => format!("<classmethod {}>", function.name),
        Value::StaticMethod(_) => "<staticmethod>".to_owned(),
        Value::Property(function) => format!("<property {}>", function.name),
        Value::Builtin(_) => "<builtin>".to_owned(),
        Value::Class(class) => format!("<class {}>", class.name),
        Value::BoundMethod(_) => "<bound method>".to_owned(),
        Value::BoundClassMethod(_) => "<bound classmethod>".to_owned(),
        Value::Instance(instance) => {
            let borrowed = instance.borrow();
            if let Some(Value::Str(msg)) = borrowed.fields.get("message") {
                msg.clone()
            } else {
                format!("<{} instance>", borrowed.class.name)
            }
        }
        Value::Module(module) => format!("<module {}>", module.borrow().name),
        Value::File(_) => "<file>".to_owned(),
        Value::Path(path) => path.to_string_lossy().to_string(),
        Value::Namespace(ns) => {
            if let Some(Value::Str(msg)) = ns.borrow().get("message") {
                msg.clone()
            } else {
                "<namespace>".to_owned()
            }
        }
        Value::ArgParser(_) => "<argparse.ArgumentParser>".to_owned(),
        Value::Super(_) => "<super>".to_owned(),
        Value::FieldSpec(_) => "<field>".to_owned(),
        Value::AutoEnum => "<enum.auto>".to_owned(),
        Value::TypingAlias(name) => name.clone(),
        Value::Range(range) => format!("range({}, {}, {})", range.start, range.stop, range.step),
        Value::Generator(_) => "<generator>".to_owned(),
    }
}

fn key_to_string(key: &Key) -> String {
    match key {
        Key::None => "None".to_owned(),
        Key::Bool(v) => v.to_string(),
        Key::Int(v) => v.to_string(),
        Key::Str(v) => format!("{v:?}"),
        Key::Tuple(items) => {
            let out = items.iter().map(key_to_string).collect::<Vec<_>>();
            format!("({})", out.join(", "))
        }
    }
}

fn display_value(value: &Value) -> String {
    value_to_string(value)
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", value_to_string(self))
    }
}

fn builtin_print(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let line = args
        .iter()
        .map(value_to_string)
        .collect::<Vec<_>>()
        .join(" ");
    println!("{line}");
    Ok(Value::None)
}

fn builtin_len(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("len() expects exactly one argument".to_owned());
    }
    let len = match &args[0] {
        Value::Str(v) => v.chars().count(),
        Value::Bytes(v) => v.borrow().len(),
        Value::List(v) => v.borrow().len(),
        Value::Dict(v) => v.borrow().len(),
        Value::Set(v) => v.borrow().len(),
        Value::Path(v) => v.to_string_lossy().len(),
        _ => return Err("object has no len()".to_owned()),
    };
    Ok(Value::Int(len as i64))
}

fn builtin_range(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let (start, stop, step) = match args.as_slice() {
        [Value::Int(stop)] => (0, *stop, 1),
        [Value::Int(start), Value::Int(stop)] => (*start, *stop, 1),
        [Value::Int(start), Value::Int(stop), Value::Int(step)] => (*start, *stop, *step),
        _ => {
            return Err("range() expects 1-3 integer arguments".to_owned());
        }
    };
    Ok(Value::Range(RangeValue { start, stop, step }))
}

fn builtin_open(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let path = match args.first() {
        Some(Value::Str(v)) => v.clone(),
        Some(Value::Path(v)) => v.to_string_lossy().to_string(),
        _ => return Err("open() expects path string as first argument".to_owned()),
    };
    let mode = if let Some(Value::Str(v)) = args.get(1) {
        v.clone()
    } else if let Some(Value::Str(v)) = kwargs.get("mode") {
        v.clone()
    } else {
        "r".to_owned()
    };

    let mut options = OpenOptions::new();
    if mode.contains('r') {
        options.read(true);
    }
    if mode.contains('w') {
        options.write(true).create(true).truncate(true);
    }
    if mode.contains('a') {
        options.write(true).create(true).append(true);
    }
    if mode.contains('+') {
        options.read(true).write(true).create(true);
    }
    let file = options
        .open(&path)
        .map_err(|err| format!("open('{path}', '{mode}') failed: {err}"))?;
    Ok(Value::File(Rc::new(RefCell::new(FileHandle { file }))))
}

fn builtin_list(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() {
        return Ok(Value::List(Rc::new(RefCell::new(Vec::new()))));
    }
    if args.len() != 1 {
        return Err("list() expects at most one argument".to_owned());
    }
    let values = interp.collect_iterable(args[0].clone())?;
    Ok(Value::List(Rc::new(RefCell::new(values))))
}

fn builtin_dict(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() {
        return Ok(Value::Dict(Rc::new(RefCell::new(BTreeMap::new()))));
    }
    if args.len() == 1 {
        if let Value::Dict(items) = &args[0] {
            return Ok(Value::Dict(Rc::new(RefCell::new(items.borrow().clone()))));
        }
    }
    Err("dict() currently supports only dict() and dict(existing_dict)".to_owned())
}

fn builtin_set(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() {
        return Ok(Value::Set(Rc::new(RefCell::new(BTreeSet::new()))));
    }
    if args.len() != 1 {
        return Err("set() expects at most one argument".to_owned());
    }
    let mut out = BTreeSet::new();
    for value in interp.collect_iterable(args[0].clone())? {
        out.insert(key_from_value(&value)?);
    }
    Ok(Value::Set(Rc::new(RefCell::new(out))))
}

fn builtin_str(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() {
        return Ok(Value::Str(String::new()));
    }
    if args.len() != 1 {
        return Err("str() expects at most one argument".to_owned());
    }
    Ok(Value::Str(value_to_string(&args[0])))
}

fn builtin_repr(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("repr() expects exactly one argument".to_owned());
    }
    let out = match &args[0] {
        Value::Str(v) => {
            let mut out = String::from("'");
            for ch in v.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    '\'' => out.push_str("\\'"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c => out.push(c),
                }
            }
            out.push('\'');
            out
        }
        other => value_to_string(other),
    };
    Ok(Value::Str(out))
}

fn builtin_int(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() {
        return Ok(Value::Int(0));
    }
    if args.len() > 2 {
        return Err("int() expects at most two arguments".to_owned());
    }
    let base = if args.len() == 2 {
        expect_int(args[1].clone())?
    } else {
        10
    };
    match &args[0] {
        Value::Int(v) => Ok(Value::Int(*v)),
        Value::Bool(v) => Ok(Value::Int(if *v { 1 } else { 0 })),
        Value::Str(v) => {
            let text = v.trim();
            if base == 10 {
                text.parse::<i64>()
                    .map(Value::Int)
                    .map_err(|_| "invalid int literal".to_owned())
            } else {
                i64::from_str_radix(text, base as u32)
                    .map(Value::Int)
                    .map_err(|_| "invalid int literal".to_owned())
            }
        }
        _ => Err("int() unsupported argument".to_owned()),
    }
}

fn builtin_bool(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() {
        return Ok(Value::Bool(false));
    }
    if args.len() != 1 {
        return Err("bool() expects at most one argument".to_owned());
    }
    Ok(Value::Bool(interp.is_truthy(&args[0])))
}

fn builtin_float(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() {
        return Ok(Value::Int(0));
    }
    if args.len() != 1 {
        return Err("float() expects at most one argument".to_owned());
    }
    match &args[0] {
        Value::Int(v) => Ok(Value::Int(*v)),
        Value::Str(v) => {
            let parsed = v
                .trim()
                .parse::<f64>()
                .map_err(|_| "invalid float literal".to_owned())?;
            Ok(Value::Int(parsed as i64))
        }
        _ => Err("float() unsupported argument".to_owned()),
    }
}

fn builtin_type(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("type() expects one argument".to_owned());
    }
    let name = match &args[0] {
        Value::None => "NoneType",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Str(_) => "str",
        Value::Bytes(_) => "bytes",
        Value::List(_) => "list",
        Value::Dict(_) => "dict",
        Value::Set(_) => "set",
        Value::Path(_) => "Path",
        Value::Class(_) => "type",
        Value::Instance(inst) => return Ok(Value::Class(inst.borrow().class.clone())),
        Value::Module(_) => "module",
        Value::Function(_)
        | Value::ClassMethod(_)
        | Value::StaticMethod(_)
        | Value::Property(_)
        | Value::Builtin(_) => "function",
        _ => "object",
    };
    let mut attrs = HashMap::new();
    attrs.insert("__name__".to_owned(), Value::Str(name.to_owned()));
    Ok(Value::Class(Rc::new(Class {
        name: name.to_owned(),
        attrs: RefCell::new(attrs),
        bases: Vec::new(),
        field_defs: Vec::new(),
        is_dataclass: false,
        is_enum_base: false,
        is_int_enum_base: false,
    })))
}

fn builtin_hasattr(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 2 {
        return Err("hasattr() expects object and name".to_owned());
    }
    let name = expect_str_arg(&args[1], "hasattr")?;
    Ok(Value::Bool(interp.get_attr(&args[0], &name)?.is_some()))
}

fn builtin_getattr(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() < 2 || args.len() > 3 {
        return Err("getattr() expects two or three arguments".to_owned());
    }
    let name = expect_str_arg(&args[1], "getattr")?;
    if let Some(v) = interp.get_attr(&args[0], &name)? {
        return Ok(v);
    }
    if let Some(default) = args.get(2) {
        Ok(default.clone())
    } else {
        Err(format!("attribute '{name}' not found"))
    }
}

fn builtin_setattr(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 3 {
        return Err("setattr() expects three arguments".to_owned());
    }
    let name = expect_str_arg(&args[1], "setattr")?;
    interp.set_attr(args[0].clone(), &name, args[2].clone())?;
    Ok(Value::None)
}

fn builtin_isinstance(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 2 {
        return Err("isinstance() expects two arguments".to_owned());
    }
    let value = &args[0];
    let cls = &args[1];
    let result = is_instance_of(value, cls);
    Ok(Value::Bool(result))
}

fn builtin_enumerate(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() || args.len() > 2 {
        return Err("enumerate() expects iterable and optional start".to_owned());
    }
    let start = if let Some(v) = args.get(1) {
        expect_int(v.clone())?
    } else {
        0
    };
    let items = interp.collect_iterable(args[0].clone())?;
    let mut out = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        out.push(Value::List(Rc::new(RefCell::new(vec![
            Value::Int(start + i as i64),
            item,
        ]))));
    }
    Ok(Value::List(Rc::new(RefCell::new(out))))
}

fn builtin_zip(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let mut all = Vec::<Vec<Value>>::new();
    for arg in args {
        all.push(interp.collect_iterable(arg)?);
    }
    let min_len = all.iter().map(Vec::len).min().unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..min_len {
        let mut row = Vec::new();
        for seq in &all {
            row.push(seq[i].clone());
        }
        out.push(Value::List(Rc::new(RefCell::new(row))));
    }
    Ok(Value::List(Rc::new(RefCell::new(out))))
}

fn builtin_sorted(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("sorted() expects one iterable".to_owned());
    }
    let mut out = interp.collect_iterable(args[0].clone())?;
    out.sort_by(|a, b| value_to_string(a).cmp(&value_to_string(b)));
    Ok(Value::List(Rc::new(RefCell::new(out))))
}

fn builtin_reversed(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("reversed() expects one iterable".to_owned());
    }
    let mut out = interp.collect_iterable(args[0].clone())?;
    out.reverse();
    Ok(Value::List(Rc::new(RefCell::new(out))))
}

fn builtin_min(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let items = if args.len() == 1 {
        interp.collect_iterable(args[0].clone())?
    } else {
        args
    };
    if items.is_empty() {
        return Err("min() arg is an empty sequence".to_owned());
    }
    let mut best = items[0].clone();
    for item in items.into_iter().skip(1) {
        if value_to_string(&item) < value_to_string(&best) {
            best = item;
        }
    }
    Ok(best)
}

fn builtin_max(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let items = if args.len() == 1 {
        interp.collect_iterable(args[0].clone())?
    } else {
        args
    };
    if items.is_empty() {
        return Err("max() arg is an empty sequence".to_owned());
    }
    let mut best = items[0].clone();
    for item in items.into_iter().skip(1) {
        if value_to_string(&item) > value_to_string(&best) {
            best = item;
        }
    }
    Ok(best)
}

fn builtin_any(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("any() expects one iterable".to_owned());
    }
    for item in interp.collect_iterable(args[0].clone())? {
        if interp.is_truthy(&item) {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}

fn builtin_all(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("all() expects one iterable".to_owned());
    }
    for item in interp.collect_iterable(args[0].clone())? {
        if !interp.is_truthy(&item) {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

fn builtin_super(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let (current_class, current_self) = if args.is_empty() {
        let Some(env) = interp.call_stack.last() else {
            return Err("super(): no current frame".to_owned());
        };
        let class_value = env
            .get("__class__")
            .ok_or_else(|| "super(): __class__ not found".to_owned())?;
        let self_value = env
            .get("__self__")
            .ok_or_else(|| "super(): __self__ not found".to_owned())?;
        (class_value, self_value)
    } else if args.len() == 2 {
        (args[0].clone(), args[1].clone())
    } else {
        return Err("super() expects zero or two arguments".to_owned());
    };

    let cls = match current_class {
        Value::Class(c) => c,
        _ => return Err("super(): first argument must be class".to_owned()),
    };
    let instance = match current_self {
        Value::Instance(i) => i,
        _ => return Err("super(): second argument must be instance".to_owned()),
    };
    let base = cls
        .bases
        .first()
        .cloned()
        .ok_or_else(|| "super(): no base class".to_owned())?;
    Ok(Value::Super(Rc::new(SuperObject {
        instance,
        class: base,
    })))
}

fn builtin_classmethod(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("classmethod() expects one argument".to_owned());
    }
    match &args[0] {
        Value::Function(function) => Ok(Value::ClassMethod(function.clone())),
        _ => Err("classmethod() expects function".to_owned()),
    }
}

fn builtin_staticmethod(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("staticmethod() expects one argument".to_owned());
    }
    Ok(Value::StaticMethod(Box::new(args[0].clone())))
}

fn builtin_property(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("property() expects one argument".to_owned());
    }
    match &args[0] {
        Value::Function(function) => Ok(Value::Property(function.clone())),
        _ => Err("property() expects function".to_owned()),
    }
}

fn int_from_bytes(
    interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() || args.len() > 3 {
        return Err("int.from_bytes() expects bytes and byteorder".to_owned());
    }
    let mut data = Vec::<u8>::new();
    match &args[0] {
        Value::Bytes(v) => data.extend(v.borrow().iter().copied()),
        other => {
            for item in interp.collect_iterable(other.clone())? {
                let byte = expect_int(item)? as u8;
                data.push(byte);
            }
        }
    }
    let byteorder = if let Some(Value::Str(v)) = args.get(1) {
        v.as_str()
    } else {
        "big"
    };
    let mut out: u64 = 0;
    if byteorder == "little" {
        for (i, b) in data.iter().enumerate() {
            out |= (*b as u64) << (8 * i);
        }
    } else {
        for b in data {
            out = (out << 8) | (b as u64);
        }
    }
    Ok(Value::Int(out as i64))
}

fn make_argparse_module() -> Value {
    let mut attrs = HashMap::new();
    attrs.insert(
        "ArgumentParser".to_owned(),
        Value::Builtin(Builtin::Native(argparse_argument_parser)),
    );
    attrs.insert(
        "Namespace".to_owned(),
        Value::Builtin(Builtin::Native(argparse_namespace_ctor)),
    );
    Value::Module(Rc::new(RefCell::new(Module {
        name: "argparse".to_owned(),
        attrs,
    })))
}

fn make_pathlib_module() -> Value {
    let mut attrs = HashMap::new();
    attrs.insert("Path".to_owned(), Value::Builtin(Builtin::Native(path_ctor)));
    Value::Module(Rc::new(RefCell::new(Module {
        name: "pathlib".to_owned(),
        attrs,
    })))
}

fn make_typing_module() -> Value {
    let mut attrs = HashMap::new();
    for name in [
        "Any", "Dict", "List", "Optional", "Set", "Tuple", "Union", "Type", "Iterable",
    ] {
        attrs.insert(name.to_owned(), Value::TypingAlias(name.to_owned()));
    }
    Value::Module(Rc::new(RefCell::new(Module {
        name: "typing".to_owned(),
        attrs,
    })))
}

fn make_copy_module() -> Value {
    let mut attrs = HashMap::new();
    attrs.insert(
        "deepcopy".to_owned(),
        Value::Builtin(Builtin::Native(copy_deepcopy)),
    );
    attrs.insert(
        "copy".to_owned(),
        Value::Builtin(Builtin::Native(copy_deepcopy)),
    );
    Value::Module(Rc::new(RefCell::new(Module {
        name: "copy".to_owned(),
        attrs,
    })))
}

fn make_abc_module() -> Value {
    let abc_class = Rc::new(Class {
        name: "ABC".to_owned(),
        attrs: RefCell::new(HashMap::new()),
        bases: Vec::new(),
        field_defs: Vec::new(),
        is_dataclass: false,
        is_enum_base: false,
        is_int_enum_base: false,
    });
    let mut attrs = HashMap::new();
    attrs.insert("ABC".to_owned(), Value::Class(abc_class));
    attrs.insert(
        "abstractmethod".to_owned(),
        Value::Builtin(Builtin::Native(abc_abstractmethod)),
    );
    Value::Module(Rc::new(RefCell::new(Module {
        name: "abc".to_owned(),
        attrs,
    })))
}

fn make_warnings_module() -> Value {
    let mut attrs = HashMap::new();
    attrs.insert("warn".to_owned(), Value::Builtin(Builtin::Native(warnings_warn)));
    Value::Module(Rc::new(RefCell::new(Module {
        name: "warnings".to_owned(),
        attrs,
    })))
}

fn make_keyword_module() -> Value {
    let mut attrs = HashMap::new();
    attrs.insert(
        "iskeyword".to_owned(),
        Value::Builtin(Builtin::Native(keyword_iskeyword)),
    );
    let kw = vec![
        "False", "None", "True", "and", "as", "assert", "break", "class", "continue",
        "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if",
        "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
        "try", "while", "with", "yield",
    ]
    .into_iter()
    .map(|s| Value::Str(s.to_owned()))
    .collect::<Vec<_>>();
    attrs.insert("kwlist".to_owned(), Value::List(Rc::new(RefCell::new(kw))));
    Value::Module(Rc::new(RefCell::new(Module {
        name: "keyword".to_owned(),
        attrs,
    })))
}

fn make_future_module() -> Value {
    let mut attrs = HashMap::new();
    attrs.insert("annotations".to_owned(), Value::Bool(true));
    Value::Module(Rc::new(RefCell::new(Module {
        name: "__future__".to_owned(),
        attrs,
    })))
}

fn argparse_argument_parser(
    _interp: &mut Interpreter,
    _args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    Ok(Value::ArgParser(Rc::new(RefCell::new(ArgParser {
        specs: Vec::new(),
    }))))
}

fn argparse_namespace_ctor(
    _interp: &mut Interpreter,
    _args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    Ok(Value::Namespace(Rc::new(RefCell::new(HashMap::new()))))
}

fn path_ctor(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() {
        return Ok(Value::Path(PathBuf::from(".")));
    }
    if args.len() != 1 {
        return Err("Path() expects one argument".to_owned());
    }
    Ok(Value::Path(path_from_value(&args[0])?))
}

fn copy_deepcopy(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("deepcopy() expects one argument".to_owned());
    }
    Ok(deep_copy_value(&args[0]))
}

fn abc_abstractmethod(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("abstractmethod expects one argument".to_owned());
    }
    Ok(args[0].clone())
}

fn warnings_warn(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if let Some(msg) = args.first() {
        eprintln!("Warning: {}", value_to_string(msg));
    }
    Ok(Value::None)
}

fn keyword_iskeyword(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() != 1 {
        return Err("iskeyword() expects one argument".to_owned());
    }
    let name = expect_str_arg(&args[0], "iskeyword")?;
    let is_kw = [
        "False", "None", "True", "and", "as", "assert", "break", "class", "continue",
        "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if",
        "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
        "try", "while", "with", "yield",
    ]
    .contains(&name.as_str());
    Ok(Value::Bool(is_kw))
}

fn make_os_module() -> Value {
    let mut attrs = HashMap::new();
    attrs.insert(
        "getcwd".to_owned(),
        Value::Builtin(Builtin::Native(os_getcwd)),
    );
    attrs.insert(
        "listdir".to_owned(),
        Value::Builtin(Builtin::Native(os_listdir)),
    );
    attrs.insert(
        "makedirs".to_owned(),
        Value::Builtin(Builtin::Native(os_makedirs)),
    );
    attrs.insert(
        "remove".to_owned(),
        Value::Builtin(Builtin::Native(os_remove)),
    );
    attrs.insert(
        "walk".to_owned(),
        Value::Builtin(Builtin::Native(os_walk)),
    );

    let mut path_attrs = HashMap::new();
    path_attrs.insert(
        "join".to_owned(),
        Value::Builtin(Builtin::Native(os_path_join)),
    );
    path_attrs.insert(
        "exists".to_owned(),
        Value::Builtin(Builtin::Native(os_path_exists)),
    );
    path_attrs.insert(
        "basename".to_owned(),
        Value::Builtin(Builtin::Native(os_path_basename)),
    );
    path_attrs.insert(
        "dirname".to_owned(),
        Value::Builtin(Builtin::Native(os_path_dirname)),
    );
    path_attrs.insert(
        "abspath".to_owned(),
        Value::Builtin(Builtin::Native(os_path_abspath)),
    );
    let path_mod = Value::Module(Rc::new(RefCell::new(Module {
        name: "os.path".to_owned(),
        attrs: path_attrs,
    })));
    attrs.insert("path".to_owned(), path_mod);

    Value::Module(Rc::new(RefCell::new(Module {
        name: "os".to_owned(),
        attrs,
    })))
}

fn make_dataclasses_module() -> Value {
    let mut attrs = HashMap::new();
    attrs.insert(
        "dataclass".to_owned(),
        Value::Builtin(Builtin::Native(dataclass_decorator)),
    );
    attrs.insert(
        "field".to_owned(),
        Value::Builtin(Builtin::Native(dataclass_field)),
    );
    Value::Module(Rc::new(RefCell::new(Module {
        name: "dataclasses".to_owned(),
        attrs,
    })))
}

fn make_enum_module() -> Value {
    let enum_class = Value::Class(Rc::new(Class {
        name: "Enum".to_owned(),
        attrs: RefCell::new(HashMap::new()),
        bases: Vec::new(),
        field_defs: Vec::new(),
        is_dataclass: false,
        is_enum_base: true,
        is_int_enum_base: false,
    }));
    let int_enum_class = Value::Class(Rc::new(Class {
        name: "IntEnum".to_owned(),
        attrs: RefCell::new(HashMap::new()),
        bases: Vec::new(),
        field_defs: Vec::new(),
        is_dataclass: false,
        is_enum_base: true,
        is_int_enum_base: true,
    }));

    let mut attrs = HashMap::new();
    attrs.insert("Enum".to_owned(), enum_class);
    attrs.insert("IntEnum".to_owned(), int_enum_class);
    attrs.insert(
        "auto".to_owned(),
        Value::Builtin(Builtin::Native(enum_auto)),
    );
    Value::Module(Rc::new(RefCell::new(Module {
        name: "enum".to_owned(),
        attrs,
    })))
}

fn make_gc_module() -> Value {
    let mut attrs = HashMap::new();
    attrs.insert(
        "collect".to_owned(),
        Value::Builtin(Builtin::Native(gc_collect)),
    );
    attrs.insert(
        "disable".to_owned(),
        Value::Builtin(Builtin::Native(gc_disable)),
    );
    attrs.insert(
        "enable".to_owned(),
        Value::Builtin(Builtin::Native(gc_enable)),
    );
    attrs.insert(
        "isenabled".to_owned(),
        Value::Builtin(Builtin::Native(gc_isenabled)),
    );
    Value::Module(Rc::new(RefCell::new(Module {
        name: "gc".to_owned(),
        attrs,
    })))
}

fn os_getcwd(
    _interp: &mut Interpreter,
    _args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let cwd = std::env::current_dir().map_err(|err| format!("getcwd failed: {err}"))?;
    Ok(Value::Str(cwd.to_string_lossy().to_string()))
}

fn os_listdir(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let path = if let Some(v) = args.first() {
        path_from_value(v)?
    } else {
        PathBuf::from(".")
    };
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&path).map_err(|err| format!("listdir failed: {err}"))? {
        let entry = entry.map_err(|err| format!("listdir failed: {err}"))?;
        names.push(Value::Str(entry.file_name().to_string_lossy().to_string()));
    }
    Ok(Value::List(Rc::new(RefCell::new(names))))
}

fn os_makedirs(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let path = match args.first() {
        Some(v) => path_from_value(v)?,
        _ => return Err("makedirs(path) expects path".to_owned()),
    };
    std::fs::create_dir_all(&path).map_err(|err| format!("makedirs failed: {err}"))?;
    Ok(Value::None)
}

fn os_remove(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let path = match args.first() {
        Some(v) => path_from_value(v)?,
        _ => return Err("remove(path) expects path".to_owned()),
    };
    std::fs::remove_file(&path).map_err(|err| format!("remove failed: {err}"))?;
    Ok(Value::None)
}

fn os_path_join(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.is_empty() {
        return Err("os.path.join expects at least one argument".to_owned());
    }
    let mut out = PathBuf::new();
    for arg in args {
        out.push(path_from_value(&arg)?);
    }
    Ok(Value::Str(out.to_string_lossy().to_string()))
}

fn os_path_exists(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let path = match args.first() {
        Some(v) => path_from_value(v)?,
        _ => return Err("os.path.exists(path) expects path".to_owned()),
    };
    Ok(Value::Bool(path.exists()))
}

fn os_path_basename(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let path = match args.first() {
        Some(v) => path_from_value(v)?,
        _ => return Err("os.path.basename(path) expects path".to_owned()),
    };
    let name = path
        .file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(Value::Str(name))
}

fn os_path_dirname(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let path = match args.first() {
        Some(v) => path_from_value(v)?,
        _ => return Err("os.path.dirname(path) expects path".to_owned()),
    };
    let name = path
        .parent()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(Value::Str(name))
}

fn os_path_abspath(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let path = match args.first() {
        Some(v) => path_from_value(v)?,
        _ => return Err("os.path.abspath(path) expects path".to_owned()),
    };
    let absolute = std::fs::canonicalize(&path).unwrap_or(path);
    Ok(Value::Str(absolute.to_string_lossy().to_string()))
}

fn os_walk(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let root = if let Some(v) = args.first() {
        path_from_value(v)?
    } else {
        PathBuf::from(".")
    };
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let mut dirnames = Vec::<Value>::new();
        let mut filenames = Vec::<Value>::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            let name = Value::Str(entry.file_name().to_string_lossy().to_string());
            if path.is_dir() {
                stack.push(path);
                dirnames.push(name);
            } else {
                filenames.push(name);
            }
        }
        out.push(Value::List(Rc::new(RefCell::new(vec![
            Value::Path(dir),
            Value::List(Rc::new(RefCell::new(dirnames))),
            Value::List(Rc::new(RefCell::new(filenames))),
        ]))));
    }
    Ok(Value::List(Rc::new(RefCell::new(out))))
}

fn dataclass_decorator(
    _interp: &mut Interpreter,
    args: Vec<Value>,
    kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    if args.len() == 1 {
        if let Value::Class(class) = &args[0] {
            class
                .attrs
                .borrow_mut()
                .insert("__dataclass__".to_owned(), Value::Bool(true));
            if let Some(frozen) = kwargs.get("frozen") {
                class
                    .attrs
                    .borrow_mut()
                    .insert("__dataclass_frozen__".to_owned(), frozen.clone());
            }
        }
        Ok(args[0].clone())
    } else {
        Ok(Value::Builtin(Builtin::Native(dataclass_decorator)))
    }
}

fn dataclass_field(
    _interp: &mut Interpreter,
    _args: Vec<Value>,
    kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    let has_default = kwargs.contains_key("default");
    let default = kwargs.get("default").cloned();
    let default_factory = kwargs.get("default_factory").cloned();
    Ok(Value::FieldSpec(Rc::new(FieldSpecValue {
        has_default,
        default,
        default_factory,
    })))
}

fn enum_auto(
    _interp: &mut Interpreter,
    _args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    Ok(Value::AutoEnum)
}

fn sys_exit(
    _interp: &mut Interpreter,
    _args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    Ok(Value::None)
}

fn gc_collect(
    _interp: &mut Interpreter,
    _args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    Ok(Value::Int(0))
}

fn gc_disable(
    _interp: &mut Interpreter,
    _args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    Ok(Value::None)
}

fn gc_enable(
    _interp: &mut Interpreter,
    _args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    Ok(Value::None)
}

fn gc_isenabled(
    _interp: &mut Interpreter,
    _args: Vec<Value>,
    _kwargs: HashMap<String, Value>,
) -> RtResult<Value> {
    Ok(Value::Bool(true))
}
