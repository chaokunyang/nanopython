#[derive(Debug, Clone)]
pub struct Program {
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Assign {
        target: Expr,
        value: Expr,
    },
    AugAssign {
        target: Expr,
        op: BinaryOp,
        value: Expr,
    },
    AnnAssign {
        target: Expr,
        value: Option<Expr>,
    },
    If {
        test: Expr,
        body: Vec<Stmt>,
        orelse: Vec<Stmt>,
    },
    While {
        test: Expr,
        body: Vec<Stmt>,
    },
    For {
        target: Expr,
        iter: Expr,
        body: Vec<Stmt>,
    },
    FunctionDef {
        name: String,
        decorators: Vec<Expr>,
        params: Vec<Param>,
        body: Vec<Stmt>,
    },
    ClassDef {
        name: String,
        decorators: Vec<Expr>,
        bases: Vec<Expr>,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>),
    Raise(Option<Expr>),
    Break,
    Continue,
    Pass,
    Import(Vec<String>),
    FromImport {
        module: String,
        names: Vec<ImportItem>,
    },
    With {
        context: Expr,
        asname: Option<String>,
        body: Vec<Stmt>,
    },
    Try {
        body: Vec<Stmt>,
        handlers: Vec<ExceptHandler>,
        orelse: Vec<Stmt>,
        finalbody: Vec<Stmt>,
    },
}

#[derive(Debug, Clone)]
pub struct ExceptHandler {
    pub exc_name: Option<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct ImportItem {
    pub name: String,
    pub asname: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub default: Option<Expr>,
    pub kind: ParamKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ParamKind {
    Positional,
    VarArgs,
    VarKwargs,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Name(String),
    Int(i64),
    Str(String),
    FStr(String),
    Bool(bool),
    None,
    List(Vec<Expr>),
    ListComp {
        elem: Box<Expr>,
        target: Box<Expr>,
        iter: Box<Expr>,
        cond: Option<Box<Expr>>,
    },
    Dict(Vec<(Expr, Expr)>),
    DictComp {
        key: Box<Expr>,
        value: Box<Expr>,
        target: Box<Expr>,
        iter: Box<Expr>,
        cond: Option<Box<Expr>>,
    },
    Set(Vec<Expr>),
    SetComp {
        elem: Box<Expr>,
        target: Box<Expr>,
        iter: Box<Expr>,
        cond: Option<Box<Expr>>,
    },
    GenComp {
        elem: Box<Expr>,
        target: Box<Expr>,
        iter: Box<Expr>,
        cond: Option<Box<Expr>>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    IfExpr {
        then_expr: Box<Expr>,
        condition: Box<Expr>,
        else_expr: Box<Expr>,
    },
    Lambda {
        params: Vec<Param>,
        body: Box<Expr>,
    },
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
        kwargs: Vec<(String, Expr)>,
    },
    Attr {
        value: Box<Expr>,
        name: String,
    },
    Subscript {
        value: Box<Expr>,
        index: Box<Expr>,
    },
    Slice {
        start: Option<Box<Expr>>,
        stop: Option<Box<Expr>>,
    },
    Starred(Box<Expr>),
    Yield(Option<Box<Expr>>),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    LShift,
    RShift,
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
    In,
    NotIn,
}
