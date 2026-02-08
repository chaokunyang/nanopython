use std::mem;

use nanopython_core::{NanoPythonError, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedModule {
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Assign {
        target: AssignTarget,
        value: Expr,
    },
    AugAssign {
        target: AssignTarget,
        op: BinaryOp,
        value: Expr,
    },
    Expr(Expr),
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    For {
        target: AssignTarget,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    Def {
        name: String,
        params: Vec<String>,
        defaults: Vec<Option<Expr>>,
        vararg: Option<String>,
        decorators: Vec<Expr>,
        body: Vec<Stmt>,
        is_generator: bool,
    },
    Class {
        name: String,
        bases: Vec<Expr>,
        decorators: Vec<Expr>,
        body: Vec<Stmt>,
    },
    With {
        expr: Expr,
        alias: Option<String>,
        body: Vec<Stmt>,
    },
    Try {
        body: Vec<Stmt>,
        handlers: Vec<ExceptHandler>,
        finally_body: Vec<Stmt>,
    },
    Return(Option<Expr>),
    Yield(Option<Expr>),
    Raise(Option<Expr>),
    Assert {
        condition: Expr,
        message: Option<Expr>,
    },
    Import {
        module: String,
    },
    FromImport {
        module: String,
        names: Vec<ImportedName>,
    },
    Pass,
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExceptHandler {
    pub exception: Option<Expr>,
    pub alias: Option<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignTarget {
    Name(String),
    Attr { object: Expr, name: String },
    Index { object: Expr, index: Expr },
    Slice {
        object: Expr,
        start: Option<Expr>,
        end: Option<Expr>,
        step: Option<Expr>,
    },
    Tuple(Vec<AssignTarget>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedName {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallArg {
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComprehensionClause {
    pub target: AssignTarget,
    pub iterable: Expr,
    pub conditions: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Name(String),
    Int(i64),
    Bool(bool),
    None,
    Str(String),
    FString(String),
    List(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Set(Vec<Expr>),
    Tuple(Vec<Expr>),
    ListComp {
        element: Box<Expr>,
        clauses: Vec<ComprehensionClause>,
    },
    DictComp {
        key: Box<Expr>,
        value: Box<Expr>,
        clauses: Vec<ComprehensionClause>,
    },
    SetComp {
        element: Box<Expr>,
        clauses: Vec<ComprehensionClause>,
    },
    GeneratorComp {
        element: Box<Expr>,
        clauses: Vec<ComprehensionClause>,
    },
    IfExpr {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
    },
    Starred(Box<Expr>),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        func: Box<Expr>,
        args: Vec<CallArg>,
    },
    Attr {
        object: Box<Expr>,
        name: String,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    Slice {
        object: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        step: Option<Box<Expr>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    BitOr,
    BitXor,
    BitAnd,
    LShift,
    RShift,
    In,
    NotIn,
    Is,
    IsNot,
    And,
    Or,
}

#[derive(Debug, Clone)]
struct SourceLine {
    line_no: usize,
    indent: usize,
    text: String,
}

pub fn parse_source(source: &str) -> Result<ParsedModule> {
    let lines = preprocess_lines(source)?;
    let mut cursor = 0;
    let body = parse_block(&lines, &mut cursor, 0)?;
    Ok(ParsedModule { body })
}

fn preprocess_lines(source: &str) -> Result<Vec<SourceLine>> {
    let mut lines = Vec::new();
    let mut pending: Option<(usize, usize, String, i32)> = None;
    let mut pending_docstring: Option<(usize, usize, &'static str)> = None;

    for (idx, raw) in source.lines().enumerate() {
        let line_no = idx + 1;
        let mut chars = raw.chars().peekable();
        let mut indent = 0usize;
        while let Some(&ch) = chars.peek() {
            if ch == ' ' {
                indent += 1;
                chars.next();
            } else if ch == '\t' {
                return Err(parse_err(line_no, "tabs are not supported; use spaces"));
            } else {
                break;
            }
        }

        let rest: String = chars.collect();

        if let Some((start_line_no, start_indent, delimiter)) = pending_docstring {
            if rest.contains(delimiter) {
                lines.push(SourceLine {
                    line_no: start_line_no,
                    indent: start_indent,
                    text: "\"\"".to_owned(),
                });
                pending_docstring = None;
            }
            continue;
        }

        let trimmed = strip_comment(&rest);
        if trimmed.trim().is_empty() {
            continue;
        }

        let piece = trimmed.trim_end().to_owned();
        if let Some(delimiter) = starts_unclosed_docstring(&piece) {
            pending_docstring = Some((line_no, indent, delimiter));
            continue;
        }
        if let Some((start_line_no, start_indent, mut accumulated, mut depth)) = pending.take() {
            let continuation = piece.trim_start();
            if !accumulated.is_empty() && !continuation.is_empty() {
                accumulated.push(' ');
            }
            accumulated.push_str(continuation);
            depth += bracket_depth_delta(continuation);
            let trailing_backslash = continuation.ends_with('\\');

            if depth > 0 || trailing_backslash {
                pending = Some((start_line_no, start_indent, accumulated, depth));
            } else {
                lines.push(SourceLine {
                    line_no: start_line_no,
                    indent: start_indent,
                    text: accumulated,
                });
            }
            continue;
        }

        let depth = bracket_depth_delta(&piece);
        let trailing_backslash = piece.ends_with('\\');
        if depth > 0 || trailing_backslash {
            pending = Some((line_no, indent, piece, depth));
        } else {
            lines.push(SourceLine {
                line_no,
                indent,
                text: piece,
            });
        }
    }

    if let Some((line_no, indent, text, _)) = pending {
        lines.push(SourceLine {
            line_no,
            indent,
            text,
        });
    }
    if pending_docstring.is_some() {
        return Err(parse_err(
            source.lines().count().max(1),
            "unterminated triple-quoted docstring",
        ));
    }

    Ok(lines)
}

fn starts_unclosed_docstring(text: &str) -> Option<&'static str> {
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("\"\"\"") {
        if !rest.contains("\"\"\"") {
            return Some("\"\"\"");
        }
    }
    if let Some(rest) = trimmed.strip_prefix("'''") {
        if !rest.contains("'''") {
            return Some("'''");
        }
    }
    None
}

fn bracket_depth_delta(text: &str) -> i32 {
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in text.chars() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                escaped = true;
            }
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ if in_single || in_double => {}
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }

    depth
}

fn strip_comment(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        match ch {
            '#' if !in_single && !in_double => break,
            '\'' if !in_double => {
                in_single = !in_single;
                out.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                out.push(ch);
            }
            '\\' => {
                out.push(ch);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            _ => out.push(ch),
        }
    }

    out
}

fn parse_block(lines: &[SourceLine], cursor: &mut usize, indent: usize) -> Result<Vec<Stmt>> {
    let mut body = Vec::new();
    let mut pending_decorators = Vec::new();

    while *cursor < lines.len() {
        let line = &lines[*cursor];

        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(parse_err(line.line_no, "unexpected indentation"));
        }

        let text = line.text.trim();
        *cursor += 1;

        if text.starts_with('@') {
            let decorator = parse_expression(&text[1..], line.line_no)?;
            pending_decorators.push(decorator);
            continue;
        }

        if let Some(condition_src) = strip_prefix_suffix(text, "if ", ":") {
            let condition = parse_expression(condition_src, line.line_no)?;
            let then_indent = next_block_indent(lines, *cursor, indent, line.line_no)?;
            let then_body = parse_block(lines, cursor, then_indent)?;
            let else_body = parse_if_trailer(lines, cursor, indent)?;

            body.push(Stmt::If {
                condition,
                then_body,
                else_body,
            });
            continue;
        }

        if let Some(condition_src) = strip_prefix_suffix(text, "while ", ":") {
            let condition = parse_expression(condition_src, line.line_no)?;
            let body_indent = next_block_indent(lines, *cursor, indent, line.line_no)?;
            let loop_body = parse_block(lines, cursor, body_indent)?;
            body.push(Stmt::While {
                condition,
                body: loop_body,
            });
            continue;
        }

        if let Some(header) = strip_prefix_suffix(text, "for ", ":") {
            let (lhs, rhs) = header.split_once(" in ").ok_or_else(|| {
                parse_err(line.line_no, "for statement must be `for name in expr:`")
            })?;
            let target = parse_assignment_target(lhs.trim(), line.line_no)?;
            let iterable = parse_expression(rhs, line.line_no)?;
            let body_indent = next_block_indent(lines, *cursor, indent, line.line_no)?;
            let loop_body = parse_block(lines, cursor, body_indent)?;
            body.push(Stmt::For {
                target,
                iterable,
                body: loop_body,
            });
            continue;
        }

        if let Some(header) = strip_prefix_suffix(text, "with ", ":") {
            let (expr_src, alias) = if let Some((expr, as_name)) = header.split_once(" as ") {
                let alias = as_name.trim();
                if !is_identifier(alias) {
                    return Err(parse_err(line.line_no, "invalid with alias"));
                }
                (expr.trim(), Some(alias.to_owned()))
            } else {
                (header.trim(), None)
            };

            let expr = parse_expression(expr_src, line.line_no)?;
            let body_indent = next_block_indent(lines, *cursor, indent, line.line_no)?;
            let with_body = parse_block(lines, cursor, body_indent)?;
            body.push(Stmt::With {
                expr,
                alias,
                body: with_body,
            });
            continue;
        }

        if text == "try:" {
            let body_indent = next_block_indent(lines, *cursor, indent, line.line_no)?;
            let try_body = parse_block(lines, cursor, body_indent)?;
            let mut handlers = Vec::new();
            let mut finally_body = Vec::new();

            while let Some(next) = lines.get(*cursor) {
                if next.indent != indent {
                    break;
                }
                let next_text = next.text.trim();
                if next_text.starts_with("except") {
                    let (exception, alias) = parse_except_header(next_text, next.line_no)?;
                    *cursor += 1;
                    let except_indent = next_block_indent(lines, *cursor, indent, next.line_no)?;
                    let except_body = parse_block(lines, cursor, except_indent)?;
                    handlers.push(ExceptHandler {
                        exception,
                        alias,
                        body: except_body,
                    });
                    continue;
                }
                if next_text == "finally:" {
                    *cursor += 1;
                    let finally_indent = next_block_indent(lines, *cursor, indent, next.line_no)?;
                    finally_body = parse_block(lines, cursor, finally_indent)?;
                }
                break;
            }

            if handlers.is_empty() && finally_body.is_empty() {
                return Err(parse_err(line.line_no, "try requires except and/or finally block"));
            }

            body.push(Stmt::Try {
                body: try_body,
                handlers,
                finally_body,
            });
            continue;
        }

        if let Some(header) = strip_prefix_suffix(text, "def ", ":") {
            let (name, params, defaults, vararg) = parse_callable_header(header, line.line_no)?;
            let body_indent = next_block_indent(lines, *cursor, indent, line.line_no)?;
            let fn_body = parse_block(lines, cursor, body_indent)?;
            let is_generator = block_has_yield(&fn_body);
            body.push(Stmt::Def {
                name,
                params,
                defaults,
                vararg,
                decorators: mem::take(&mut pending_decorators),
                body: fn_body,
                is_generator,
            });
            continue;
        }

        if let Some(header) = strip_prefix_suffix(text, "class ", ":") {
            let (name, bases) = parse_class_header(header, line.line_no)?;
            let body_indent = next_block_indent(lines, *cursor, indent, line.line_no)?;
            let class_body = parse_block(lines, cursor, body_indent)?;
            body.push(Stmt::Class {
                name,
                bases,
                decorators: mem::take(&mut pending_decorators),
                body: class_body,
            });
            continue;
        }

        if let Some(stmt) = parse_simple_stmt(text, line.line_no)? {
            body.push(stmt);
            continue;
        }

        return Err(parse_err(
            line.line_no,
            "unsupported statement syntax in custom backend",
        ));
    }

    if !pending_decorators.is_empty() {
        return Err(parse_err(
            lines
                .last()
                .map(|line| line.line_no)
                .unwrap_or_default()
                .max(1),
            "dangling decorator without class/def",
        ));
    }

    Ok(body)
}

fn parse_if_trailer(lines: &[SourceLine], cursor: &mut usize, indent: usize) -> Result<Vec<Stmt>> {
    let Some(next) = lines.get(*cursor) else {
        return Ok(Vec::new());
    };
    if next.indent != indent {
        return Ok(Vec::new());
    }
    let text = next.text.trim();
    if text == "else:" {
        *cursor += 1;
        let else_indent = next_block_indent(lines, *cursor, indent, next.line_no)?;
        return parse_block(lines, cursor, else_indent);
    }
    if let Some(condition_src) = strip_prefix_suffix(text, "elif ", ":") {
        *cursor += 1;
        let condition = parse_expression(condition_src, next.line_no)?;
        let then_indent = next_block_indent(lines, *cursor, indent, next.line_no)?;
        let then_body = parse_block(lines, cursor, then_indent)?;
        let else_body = parse_if_trailer(lines, cursor, indent)?;
        return Ok(vec![Stmt::If {
            condition,
            then_body,
            else_body,
        }]);
    }
    Ok(Vec::new())
}

fn parse_except_header(text: &str, line_no: usize) -> Result<(Option<Expr>, Option<String>)> {
    let rest = text
        .strip_prefix("except")
        .ok_or_else(|| parse_err(line_no, "invalid except header"))?
        .trim();
    let rest = rest
        .strip_suffix(':')
        .ok_or_else(|| parse_err(line_no, "except header must end with `:`"))?
        .trim();
    if rest.is_empty() {
        return Ok((None, None));
    }

    let (exc_src, alias) = split_except_alias(rest);
    let exception = parse_expression(exc_src, line_no)?;
    let alias = if let Some(alias) = alias {
        if !is_identifier(alias) {
            return Err(parse_err(line_no, "invalid except alias"));
        }
        Some(alias.to_owned())
    } else {
        None
    };
    Ok((Some(exception), alias))
}

fn split_except_alias(rest: &str) -> (&str, Option<&str>) {
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0usize;
    while i + 3 < chars.len() {
        let ch = chars[i];
        match ch {
            '\\' => i += 1,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' | '[' | '{' if !in_single && !in_double => depth += 1,
            ')' | ']' | '}' if !in_single && !in_double => depth -= 1,
            ' ' if !in_single && !in_double && depth == 0 => {
                if chars.get(i + 1) == Some(&'a')
                    && chars.get(i + 2) == Some(&'s')
                    && chars.get(i + 3) == Some(&' ')
                {
                    let left = rest[..i].trim();
                    let right = rest[i + 4..].trim();
                    if !left.is_empty() && !right.is_empty() {
                        return (left, Some(right));
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    (rest, None)
}

fn parse_simple_stmt(text: &str, line_no: usize) -> Result<Option<Stmt>> {
    if text.starts_with("\"\"\"") && text.ends_with("\"\"\"") && text.len() >= 6 {
        let inner = text[3..text.len() - 3].to_owned();
        return Ok(Some(Stmt::Expr(Expr::Str(inner))));
    }
    if text.starts_with("'''") && text.ends_with("'''") && text.len() >= 6 {
        let inner = text[3..text.len() - 3].to_owned();
        return Ok(Some(Stmt::Expr(Expr::Str(inner))));
    }

    match text {
        "pass" => return Ok(Some(Stmt::Pass)),
        "break" => return Ok(Some(Stmt::Break)),
        "continue" => return Ok(Some(Stmt::Continue)),
        _ => {}
    }

    if text == "return" || text.starts_with("return ") {
        let rest = text["return".len()..].trim();
        if rest.is_empty() {
            return Ok(Some(Stmt::Return(None)));
        }
        return Ok(Some(Stmt::Return(Some(parse_expression(rest, line_no)?))));
    }

    if text == "yield" || text.starts_with("yield ") {
        let rest = text["yield".len()..].trim();
        if rest.is_empty() {
            return Ok(Some(Stmt::Yield(None)));
        }
        return Ok(Some(Stmt::Yield(Some(parse_expression(rest, line_no)?))));
    }

    if text == "raise" || text.starts_with("raise ") {
        let rest = text["raise".len()..].trim();
        if rest.is_empty() {
            return Ok(Some(Stmt::Raise(None)));
        }
        let expr_src = split_raise_from(rest).0;
        return Ok(Some(Stmt::Raise(Some(parse_expression(expr_src, line_no)?))));
    }

    if let Some(rest) = text.strip_prefix("assert ") {
        let (cond_src, msg_src) = split_top_level_comma(rest);
        let condition = parse_expression(cond_src.trim(), line_no)?;
        let message = msg_src
            .map(str::trim)
            .filter(|src| !src.is_empty())
            .map(|src| parse_expression(src, line_no))
            .transpose()?;
        return Ok(Some(Stmt::Assert { condition, message }));
    }

    if let Some(rest) = text.strip_prefix("import ") {
        let module = rest.trim();
        if module.is_empty() || !module.split('.').all(is_identifier) {
            return Err(parse_err(line_no, "invalid import module name"));
        }
        return Ok(Some(Stmt::Import {
            module: module.to_owned(),
        }));
    }

    if let Some(rest) = text.strip_prefix("from ") {
        let (module, imported) = rest
            .split_once(" import ")
            .ok_or_else(|| parse_err(line_no, "invalid from-import syntax"))?;
        let module = module.trim();
        let mut imported = imported.trim();
        if module.is_empty() || !module.split('.').all(is_identifier) {
            return Err(parse_err(line_no, "invalid module name in from-import"));
        }
        if imported.is_empty() {
            return Err(parse_err(line_no, "invalid imported names in from-import"));
        }
        if imported.starts_with('(') && imported.ends_with(')') && imported.len() >= 2 {
            imported = imported[1..imported.len() - 1].trim();
        }
        let mut names = Vec::new();
        for item in split_top_level_commas(imported) {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            let (name, alias) = if let Some((name, alias)) = item.split_once(" as ") {
                (name.trim(), Some(alias.trim()))
            } else {
                (item, None)
            };

            if name != "*" && !is_identifier(name) {
                return Err(parse_err(line_no, "invalid imported name in from-import"));
            }
            if let Some(alias) = alias {
                if !is_identifier(alias) {
                    return Err(parse_err(line_no, "invalid alias in from-import"));
                }
            }
            names.push(ImportedName {
                name: name.to_owned(),
                alias: alias.map(ToOwned::to_owned),
            });
        }
        return Ok(Some(Stmt::FromImport {
            module: module.to_owned(),
            names,
        }));
    }

    if let Some(eq_idx) = find_assignment(text) {
        let left = text[..eq_idx].trim();
        let right = text[eq_idx + 1..].trim();
        if right.is_empty() {
            return Err(parse_err(line_no, "assignment right side is empty"));
        }

        if let Some(colon_idx) = find_annotation_split(left) {
            let target = parse_assignment_target(left[..colon_idx].trim(), line_no)?;
            let value = parse_expression(right, line_no)?;
            return Ok(Some(Stmt::Assign { target, value }));
        }

        let target = parse_assignment_target(left, line_no)?;

        let value = parse_expression(right, line_no)?;
        return Ok(Some(Stmt::Assign { target, value }));
    }

    if let Some((idx, width, op)) = find_aug_assignment(text) {
        let left = text[..idx].trim();
        let right = text[idx + width..].trim();
        if right.is_empty() {
            return Err(parse_err(line_no, "augmented assignment right side is empty"));
        }
        let target = parse_assignment_target(left, line_no)?;
        if matches!(target, AssignTarget::Tuple(_)) {
            return Err(parse_err(
                line_no,
                "augmented assignment target cannot be a tuple/list",
            ));
        }
        let value = parse_expression(right, line_no)?;
        return Ok(Some(Stmt::AugAssign { target, op, value }));
    }

    if let Some(colon_idx) = find_annotation_split(text) {
        let target_src = text[..colon_idx].trim();
        if parse_assignment_target(target_src, line_no).is_ok() {
            let target = parse_assignment_target(target_src, line_no)?;
            return Ok(Some(Stmt::Assign {
                target,
                value: Expr::None,
            }));
        }
    }

    if text.is_empty() {
        return Ok(None);
    }

    Ok(Some(Stmt::Expr(parse_expression(text, line_no)?)))
}

fn find_assignment(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = text.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '\\' => i += 1,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' | '[' | '{' if !in_single && !in_double => depth += 1,
            ')' | ']' | '}' if !in_single && !in_double => depth -= 1,
            '=' if !in_single && !in_double && depth == 0 => {
                let prev = i.checked_sub(1).and_then(|idx| chars.get(idx));
                let next = chars.get(i + 1);
                if prev == Some(&'=')
                    || next == Some(&'=')
                    || prev == Some(&'!')
                    || prev == Some(&'+')
                    || prev == Some(&'-')
                    || prev == Some(&'*')
                    || prev == Some(&'/')
                    || prev == Some(&'%')
                    || prev == Some(&'&')
                    || prev == Some(&'|')
                    || prev == Some(&'^')
                    || prev == Some(&'<')
                    || prev == Some(&'>')
                    || prev == Some(&':')
                {
                    i += 1;
                    continue;
                }
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }

    None
}

fn find_aug_assignment(text: &str) -> Option<(usize, usize, BinaryOp)> {
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;

    while i + 1 < chars.len() {
        let ch = chars[i];
        match ch {
            '\\' => i += 1,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' | '[' | '{' if !in_single && !in_double => depth += 1,
            ')' | ']' | '}' if !in_single && !in_double => depth -= 1,
            '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^' | '<' | '>'
                if !in_single && !in_double && depth == 0 =>
            {
                if ch == '/' && chars.get(i + 1) == Some(&'/') && chars.get(i + 2) == Some(&'=') {
                    return Some((i, 3, BinaryOp::FloorDiv));
                }
                if ch == '<' && chars.get(i + 1) == Some(&'<') && chars.get(i + 2) == Some(&'=') {
                    return Some((i, 3, BinaryOp::LShift));
                }
                if ch == '>' && chars.get(i + 1) == Some(&'>') && chars.get(i + 2) == Some(&'=') {
                    return Some((i, 3, BinaryOp::RShift));
                }
                if chars[i + 1] == '=' {
                    let op = match ch {
                        '+' => BinaryOp::Add,
                        '-' => BinaryOp::Sub,
                        '*' => BinaryOp::Mul,
                        '/' => BinaryOp::Div,
                        '%' => BinaryOp::Mod,
                        '&' => BinaryOp::BitAnd,
                        '|' => BinaryOp::BitOr,
                        '^' => BinaryOp::BitXor,
                        _ => unreachable!(),
                    };
                    return Some((i, 2, op));
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
}

fn find_annotation_split(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = text.chars().collect();

    for (idx, ch) in chars.iter().enumerate() {
        match *ch {
            '\\' => continue,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' | '[' | '{' if !in_single && !in_double => depth += 1,
            ')' | ']' | '}' if !in_single && !in_double => depth -= 1,
            ':' if !in_single && !in_double && depth == 0 => return Some(idx),
            _ => {}
        }
    }

    None
}

fn split_top_level_comma(text: &str) -> (&str, Option<&str>) {
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '\\' => i += 1,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' | '[' | '{' if !in_single && !in_double => depth += 1,
            ')' | ']' | '}' if !in_single && !in_double => depth -= 1,
            ',' if !in_single && !in_double && depth == 0 => {
                let left = text[..i].trim();
                let right = text[i + 1..].trim();
                return (left, Some(right));
            }
            _ => {}
        }
        i += 1;
    }
    (text.trim(), None)
}

fn split_raise_from(text: &str) -> (&str, Option<&str>) {
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i + 5 < chars.len() {
        let ch = chars[i];
        match ch {
            '\\' => i += 1,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' | '[' | '{' if !in_single && !in_double => depth += 1,
            ')' | ']' | '}' if !in_single && !in_double => depth -= 1,
            ' ' if !in_single && !in_double && depth == 0 => {
                if chars.get(i + 1) == Some(&'f')
                    && chars.get(i + 2) == Some(&'r')
                    && chars.get(i + 3) == Some(&'o')
                    && chars.get(i + 4) == Some(&'m')
                    && chars.get(i + 5) == Some(&' ')
                {
                    let left = text[..i].trim();
                    let right = text[i + 6..].trim();
                    if !left.is_empty() && !right.is_empty() {
                        return (left, Some(right));
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    (text, None)
}

fn split_top_level_commas(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = text.chars().collect();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '\\' => i += 1,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' | '[' | '{' if !in_single && !in_double => depth += 1,
            ')' | ']' | '}' if !in_single && !in_double => depth -= 1,
            ',' if !in_single && !in_double && depth == 0 => {
                out.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(text[start..].trim());
    out
}

fn parse_assignment_target(text: &str, line_no: usize) -> Result<AssignTarget> {
    let items = split_top_level_commas(text);
    if items.len() > 1 {
        let mut targets = Vec::new();
        for item in items {
            if item.is_empty() {
                return Err(parse_err(line_no, "invalid assignment target"));
            }
            targets.push(parse_assignment_target(item, line_no)?);
        }
        return Ok(AssignTarget::Tuple(targets));
    }

    let expr = parse_expression(text, line_no)?;
    expr_to_assign_target(expr, line_no)
}

fn expr_to_assign_target(expr: Expr, line_no: usize) -> Result<AssignTarget> {
    match expr {
        Expr::Name(name) => Ok(AssignTarget::Name(name)),
        Expr::Attr { object, name } => Ok(AssignTarget::Attr {
            object: *object,
            name,
        }),
        Expr::Index { object, index } => Ok(AssignTarget::Index {
            object: *object,
            index: *index,
        }),
        Expr::Slice {
            object,
            start,
            end,
            step,
        } => Ok(AssignTarget::Slice {
            object: *object,
            start: start.map(|v| *v),
            end: end.map(|v| *v),
            step: step.map(|v| *v),
        }),
        Expr::Tuple(items) | Expr::List(items) => {
            let mut targets = Vec::new();
            for item in items {
                targets.push(expr_to_assign_target(item, line_no)?);
            }
            Ok(AssignTarget::Tuple(targets))
        }
        _ => Err(parse_err(line_no, "invalid assignment target")),
    }
}

fn parse_callable_header(
    header: &str,
    line_no: usize,
) -> Result<(String, Vec<String>, Vec<Option<Expr>>, Option<String>)> {
    let open = header
        .find('(')
        .ok_or_else(|| parse_err(line_no, "missing `(` in function definition"))?;
    let close = header
        .rfind(')')
        .ok_or_else(|| parse_err(line_no, "missing `)` in function definition"))?;

    let name = header[..open].trim();
    if !is_identifier(name) {
        return Err(parse_err(line_no, "invalid function name"));
    }

    let params_src = header[open + 1..close].trim();
    let (params, defaults, vararg) = if params_src.is_empty() {
        (Vec::new(), Vec::new(), None)
    } else {
        let mut parsed_params = Vec::new();
        let mut parsed_defaults = Vec::new();
        let mut parsed_vararg = None;
        for item in split_top_level_commas(params_src) {
            let mut source = item.trim();
            if source.is_empty() || source == "*" || source == "/" {
                continue;
            }
            let mut is_vararg = false;
            if let Some(stripped) = source.strip_prefix("**") {
                source = stripped.trim();
            } else if let Some(stripped) = source.strip_prefix('*') {
                source = stripped.trim();
                is_vararg = true;
            }

            let default = if let Some((before_eq, after_eq)) = source.split_once('=') {
                source = before_eq.trim();
                Some(parse_expression(after_eq.trim(), line_no)?)
            } else {
                None
            };
            if let Some((before_colon, _)) = source.split_once(':') {
                source = before_colon.trim();
            }

            if !is_identifier(source) {
                return Err(parse_err(line_no, "invalid function parameter"));
            }
            if is_vararg {
                parsed_vararg = Some(source.to_owned());
                continue;
            }
            parsed_params.push(source.to_owned());
            parsed_defaults.push(default);
        }
        (parsed_params, parsed_defaults, parsed_vararg)
    };

    Ok((name.to_owned(), params, defaults, vararg))
}

fn parse_class_header(header: &str, line_no: usize) -> Result<(String, Vec<Expr>)> {
    let name = header.split('(').next().unwrap_or(header).trim();
    if !is_identifier(name) {
        return Err(parse_err(line_no, "invalid class name"));
    }

    let mut bases = Vec::new();
    if let Some(open) = header.find('(') {
        let close = header
            .rfind(')')
            .ok_or_else(|| parse_err(line_no, "missing `)` in class definition"))?;
        let bases_src = header[open + 1..close].trim();
        if !bases_src.is_empty() {
            for item in split_top_level_commas(bases_src) {
                let item = item.trim();
                if item.is_empty() {
                    continue;
                }
                bases.push(parse_expression(item, line_no)?);
            }
        }
    }
    Ok((name.to_owned(), bases))
}

fn block_has_yield(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        match stmt {
            Stmt::Yield(_) => return true,
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                if block_has_yield(then_body) || block_has_yield(else_body) {
                    return true;
                }
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::With { body, .. }
            | Stmt::Class { body, .. } => {
                if block_has_yield(body) {
                    return true;
                }
            }
            Stmt::Try {
                body,
                handlers,
                finally_body,
            } => {
                if block_has_yield(body) || block_has_yield(finally_body) {
                    return true;
                }
                for handler in handlers {
                    if block_has_yield(&handler.body) {
                        return true;
                    }
                }
            }
            Stmt::Def { .. } => {}
            _ => {}
        }
    }

    false
}

fn next_block_indent(
    lines: &[SourceLine],
    cursor: usize,
    current_indent: usize,
    line_no: usize,
) -> Result<usize> {
    let next = lines
        .get(cursor)
        .ok_or_else(|| parse_err(line_no, "expected indented block"))?;

    if next.indent <= current_indent {
        return Err(parse_err(next.line_no, "expected indented block"));
    }

    Ok(next.indent)
}

fn strip_prefix_suffix<'a>(text: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    if !text.starts_with(prefix) || !text.ends_with(suffix) {
        return None;
    }
    Some(text[prefix.len()..text.len() - suffix.len()].trim())
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

pub fn parse_expression(source: &str, line_no: usize) -> Result<Expr> {
    let mut top_level_parts = split_top_level_commas(source.trim());
    if top_level_parts.len() > 1 {
        if matches!(top_level_parts.last(), Some(part) if part.is_empty()) {
            top_level_parts.pop();
        }
        let mut items = Vec::new();
        for part in top_level_parts {
            if part.is_empty() {
                return Err(parse_err(line_no, "invalid tuple expression"));
            }
            items.push(parse_expression(part, line_no)?);
        }
        return Ok(Expr::Tuple(items));
    }

    let tokens = tokenize(source, line_no)?;
    let mut parser = ExprParser::new(tokens, line_no);
    let expr = parser.parse_full_expr()?;
    if !matches!(parser.peek(), Token::Eof) {
        return Err(parse_err(line_no, "unexpected token at end of expression"));
    }
    Ok(expr)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Name(String),
    Int(i64),
    Str(String),
    FString(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    FloorDiv,
    Percent,
    Assign,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Pipe,
    Caret,
    Amp,
    Shl,
    Shr,
    In,
    Is,
    Lambda,
    For,
    IfKw,
    ElseKw,
    And,
    Or,
    Not,
    Eof,
}

fn tokenize(source: &str, line_no: usize) -> Result<Vec<Token>> {
    let mut chars = source.chars().peekable();
    let mut tokens = Vec::new();

    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_whitespace() {
            chars.next();
            continue;
        }

        if ch.is_ascii_digit() {
            let mut num = String::new();
            let first = chars
                .next()
                .ok_or_else(|| parse_err(line_no, "invalid integer literal"))?;
            num.push(first);
            if first == '0' && matches!(chars.peek(), Some('x' | 'X')) {
                let marker = chars
                    .next()
                    .ok_or_else(|| parse_err(line_no, "invalid hex literal"))?;
                num.push(marker);
                while let Some(&digit) = chars.peek() {
                    if digit.is_ascii_hexdigit() {
                        num.push(digit);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let value = i64::from_str_radix(num.trim_start_matches("0x").trim_start_matches("0X"), 16)
                    .map_err(|_| parse_err(line_no, "invalid hex literal"))?;
                tokens.push(Token::Int(value));
            } else {
                while let Some(&digit) = chars.peek() {
                    if digit.is_ascii_digit() {
                        num.push(digit);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let value = num
                    .parse::<i64>()
                    .map_err(|_| parse_err(line_no, "invalid integer literal"))?;
                tokens.push(Token::Int(value));
            }
            continue;
        }

        if ch == '\'' || ch == '"' {
            let quote = chars
                .next()
                .ok_or_else(|| parse_err(line_no, "expected quote"))?;
            let string = read_string_literal(&mut chars, quote, false, line_no)?;
            tokens.push(Token::Str(string));
            continue;
        }

        if ch == '_' || ch.is_ascii_alphabetic() {
            let mut ident = String::new();
            while let Some(&c) = chars.peek() {
                if c == '_' || c.is_ascii_alphanumeric() {
                    ident.push(c);
                    chars.next();
                } else {
                    break;
                }
            }

            if let Some(&quote) = chars.peek() {
                if (quote == '\'' || quote == '"') && is_string_prefix(&ident) {
                    chars.next();
                    let lower = ident.to_ascii_lowercase();
                    let is_raw = lower.contains('r');
                    let is_f = lower.contains('f');
                    let content = read_string_literal(&mut chars, quote, is_raw, line_no)?;
                    if is_f {
                        tokens.push(Token::FString(content));
                    } else {
                        tokens.push(Token::Str(content));
                    }
                    continue;
                }
            }

            match ident.as_str() {
                "and" => tokens.push(Token::And),
                "or" => tokens.push(Token::Or),
                "not" => tokens.push(Token::Not),
                "in" => tokens.push(Token::In),
                "is" => tokens.push(Token::Is),
                "lambda" => tokens.push(Token::Lambda),
                "for" => tokens.push(Token::For),
                "if" => tokens.push(Token::IfKw),
                "else" => tokens.push(Token::ElseKw),
                _ => tokens.push(Token::Name(ident)),
            }
            continue;
        }

        let token = match ch {
            '(' => Token::LParen,
            ')' => Token::RParen,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            ',' => Token::Comma,
            ':' => Token::Colon,
            '.' => Token::Dot,
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => {
                chars.next();
                if chars.next_if_eq(&'/').is_some() {
                    tokens.push(Token::FloorDiv);
                } else {
                    tokens.push(Token::Slash);
                }
                continue;
            }
            '%' => Token::Percent,
            '&' => Token::Amp,
            '|' => Token::Pipe,
            '^' => Token::Caret,
            '=' => {
                chars.next();
                if chars.next_if_eq(&'=').is_some() {
                    tokens.push(Token::EqEq);
                    continue;
                }
                tokens.push(Token::Assign);
                continue;
            }
            '!' => {
                chars.next();
                if chars.next_if_eq(&'=').is_some() {
                    tokens.push(Token::NotEq);
                    continue;
                }
                return Err(parse_err(line_no, "unexpected `!` in expression"));
            }
            '<' => {
                chars.next();
                if chars.next_if_eq(&'=').is_some() {
                    tokens.push(Token::Le);
                } else if chars.next_if_eq(&'<').is_some() {
                    tokens.push(Token::Shl);
                } else {
                    tokens.push(Token::Lt);
                }
                continue;
            }
            '>' => {
                chars.next();
                if chars.next_if_eq(&'=').is_some() {
                    tokens.push(Token::Ge);
                } else if chars.next_if_eq(&'>').is_some() {
                    tokens.push(Token::Shr);
                } else {
                    tokens.push(Token::Gt);
                }
                continue;
            }
            _ => {
                return Err(parse_err(
                    line_no,
                    &format!("unexpected character in expression: `{ch}`"),
                ));
            }
        };

        chars.next();
        tokens.push(token);
    }

    tokens.push(Token::Eof);
    Ok(tokens)
}

fn is_string_prefix(prefix: &str) -> bool {
    if prefix.is_empty() || prefix.len() > 2 {
        return false;
    }
    prefix
        .chars()
        .all(|ch| matches!(ch, 'f' | 'F' | 'r' | 'R' | 'b' | 'B' | 'u' | 'U'))
}

fn read_string_literal(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    quote: char,
    raw: bool,
    line_no: usize,
) -> Result<String> {
    let mut out = String::new();
    let triple = if chars.peek() == Some(&quote) {
        let mut clone = chars.clone();
        clone.next();
        clone.peek() == Some(&quote)
    } else {
        false
    };

    if triple {
        chars.next();
        chars.next();
    }

    loop {
        let next = chars
            .next()
            .ok_or_else(|| parse_err(line_no, "unterminated string literal"))?;
        if next == quote {
            if triple {
                if chars.peek() == Some(&quote) {
                    let mut clone = chars.clone();
                    clone.next();
                    if clone.peek() == Some(&quote) {
                        chars.next();
                        chars.next();
                        break;
                    }
                }
                out.push(next);
                continue;
            }
            break;
        }
        if next == '\\' && !raw {
            let escaped = chars
                .next()
                .ok_or_else(|| parse_err(line_no, "unterminated escape sequence"))?;
            let mapped = match escaped {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                other => other,
            };
            out.push(mapped);
            continue;
        }
        out.push(next);
    }

    Ok(out)
}

struct ExprParser {
    tokens: Vec<Token>,
    pos: usize,
    line_no: usize,
}

impl ExprParser {
    fn new(tokens: Vec<Token>, line_no: usize) -> Self {
        Self {
            tokens,
            pos: 0,
            line_no,
        }
    }

    fn parse_full_expr(&mut self) -> Result<Expr> {
        self.parse_if_expr()
    }

    fn parse_if_expr(&mut self) -> Result<Expr> {
        if matches!(self.peek(), Token::Lambda) {
            return self.parse_lambda_expr();
        }

        let then_expr = self.parse_expr(0)?;
        if matches!(self.peek(), Token::IfKw) {
            self.bump();
            let condition = self.parse_expr(0)?;
            self.expect(Token::ElseKw, "expected `else` in conditional expression")?;
            let else_expr = self.parse_if_expr()?;
            Ok(Expr::IfExpr {
                condition: Box::new(condition),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            })
        } else {
            Ok(then_expr)
        }
    }

    fn parse_lambda_expr(&mut self) -> Result<Expr> {
        self.expect(Token::Lambda, "expected `lambda`")?;
        let mut params = Vec::new();
        if !matches!(self.peek(), Token::Colon) {
            loop {
                let param = match self.bump() {
                    Token::Name(name) => name,
                    token => {
                        return Err(parse_err(
                            self.line_no,
                            &format!("invalid lambda parameter token: {token:?}"),
                        ));
                    }
                };
                params.push(param);
                if matches!(self.peek(), Token::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect(Token::Colon, "expected `:` after lambda parameters")?;
        let body = self.parse_full_expr()?;
        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
        })
    }

    fn parse_expr(&mut self, min_prec: u8) -> Result<Expr> {
        let mut left = self.parse_unary()?;

        loop {
            let (op, prec, width) = if matches!(self.peek(), Token::Not)
                && matches!(self.peek_n(1), Token::In)
            {
                (BinaryOp::NotIn, 3, 2)
            } else if matches!(self.peek(), Token::Is) && matches!(self.peek_n(1), Token::Not) {
                (BinaryOp::IsNot, 3, 2)
            } else {
                match self.peek() {
                    Token::Or => (BinaryOp::Or, 1, 1),
                    Token::And => (BinaryOp::And, 2, 1),
                    Token::EqEq => (BinaryOp::Eq, 3, 1),
                    Token::NotEq => (BinaryOp::Ne, 3, 1),
                    Token::Lt => (BinaryOp::Lt, 3, 1),
                    Token::Le => (BinaryOp::Le, 3, 1),
                    Token::Gt => (BinaryOp::Gt, 3, 1),
                    Token::Ge => (BinaryOp::Ge, 3, 1),
                    Token::In => (BinaryOp::In, 3, 1),
                    Token::Is => (BinaryOp::Is, 3, 1),
                    Token::Pipe => (BinaryOp::BitOr, 4, 1),
                    Token::Caret => (BinaryOp::BitXor, 5, 1),
                    Token::Amp => (BinaryOp::BitAnd, 6, 1),
                    Token::Shl => (BinaryOp::LShift, 7, 1),
                    Token::Shr => (BinaryOp::RShift, 7, 1),
                    Token::Plus => (BinaryOp::Add, 8, 1),
                    Token::Minus => (BinaryOp::Sub, 8, 1),
                    Token::Star => (BinaryOp::Mul, 9, 1),
                    Token::Slash => (BinaryOp::Div, 9, 1),
                    Token::FloorDiv => (BinaryOp::FloorDiv, 9, 1),
                    Token::Percent => (BinaryOp::Mod, 9, 1),
                    _ => break,
                }
            };

            if prec < min_prec {
                break;
            }

            for _ in 0..width {
                self.bump();
            }
            let right = self.parse_expr(prec + 1)?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        match self.peek() {
            Token::Minus => {
                self.bump();
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            Token::Not => {
                self.bump();
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            Token::Star => {
                self.bump();
                Ok(Expr::Starred(Box::new(self.parse_unary()?)))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek() {
                Token::Dot => {
                    self.bump();
                    let name = match self.bump() {
                        Token::Name(name) => name,
                        _ => return Err(parse_err(self.line_no, "expected identifier after `.`")),
                    };
                    expr = Expr::Attr {
                        object: Box::new(expr),
                        name,
                    };
                }
                Token::LParen => {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        loop {
                            if let (Token::Name(name), Token::Assign) =
                                (self.peek(), self.peek_n(1))
                            {
                                self.bump();
                                self.bump();
                                args.push(CallArg {
                                    name: Some(name),
                                    value: self.parse_argument_expr()?,
                                });
                            } else {
                                args.push(CallArg {
                                    name: None,
                                    value: self.parse_argument_expr()?,
                                });
                            }
                            if matches!(self.peek(), Token::Comma) {
                                self.bump();
                                if matches!(self.peek(), Token::RParen) {
                                    break;
                                }
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect(Token::RParen, "expected `)` after call arguments")?;
                    expr = Expr::Call {
                        func: Box::new(expr),
                        args,
                    };
                }
                Token::LBracket => {
                    self.bump();
                    if matches!(self.peek(), Token::Colon) {
                        self.bump();
                        let (end, step) = self.parse_slice_tail()?;
                        self.expect(Token::RBracket, "expected `]` after slice")?;
                        expr = Expr::Slice {
                            object: Box::new(expr),
                            start: None,
                            end: end.map(Box::new),
                            step: step.map(Box::new),
                        };
                        continue;
                    }

                    let first = self.parse_full_expr()?;
                    if matches!(self.peek(), Token::Colon) {
                        self.bump();
                        let (end, step) = self.parse_slice_tail()?;
                        self.expect(Token::RBracket, "expected `]` after slice")?;
                        expr = Expr::Slice {
                            object: Box::new(expr),
                            start: Some(Box::new(first)),
                            end: end.map(Box::new),
                            step: step.map(Box::new),
                        };
                    } else {
                        let index = if matches!(self.peek(), Token::Comma) {
                            let mut items = vec![first];
                            while matches!(self.peek(), Token::Comma) {
                                self.bump();
                                if matches!(self.peek(), Token::RBracket) {
                                    break;
                                }
                                items.push(self.parse_full_expr()?);
                            }
                            Expr::Tuple(items)
                        } else {
                            first
                        };
                        self.expect(Token::RBracket, "expected `]` after index")?;
                        expr = Expr::Index {
                            object: Box::new(expr),
                            index: Box::new(index),
                        };
                    }
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.bump() {
            Token::Name(name) => match name.as_str() {
                "True" => Ok(Expr::Bool(true)),
                "False" => Ok(Expr::Bool(false)),
                "None" => Ok(Expr::None),
                _ => Ok(Expr::Name(name)),
            },
            Token::Int(value) => Ok(Expr::Int(value)),
            Token::Str(value) => Ok(self.parse_adjacent_string_literals(Expr::Str(value))),
            Token::FString(value) => {
                Ok(self.parse_adjacent_string_literals(Expr::FString(value)))
            }
            Token::LParen => {
                if matches!(self.peek(), Token::RParen) {
                    self.bump();
                    return Ok(Expr::Tuple(vec![]));
                }

                let first = self.parse_full_expr()?;
                if matches!(self.peek(), Token::For) {
                    let clauses = self.parse_comprehension_clauses()?;
                    self.expect(Token::RParen, "expected `)` after generator expression")?;
                    Ok(Expr::GeneratorComp {
                        element: Box::new(first),
                        clauses,
                    })
                } else if matches!(self.peek(), Token::Comma) {
                    let mut items = vec![first];
                    while matches!(self.peek(), Token::Comma) {
                        self.bump();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                        items.push(self.parse_full_expr()?);
                    }
                    self.expect(Token::RParen, "expected `)` after tuple")?;
                    Ok(Expr::Tuple(items))
                } else {
                    self.expect(Token::RParen, "expected `)`")?;
                    Ok(first)
                }
            }
            Token::LBracket => {
                let mut items = Vec::new();
                if !matches!(self.peek(), Token::RBracket) {
                    let first = self.parse_full_expr()?;
                    if matches!(self.peek(), Token::For) {
                        let clauses = self.parse_comprehension_clauses()?;
                        self.expect(Token::RBracket, "expected `]` after list comprehension")?;
                        return Ok(Expr::ListComp {
                            element: Box::new(first),
                            clauses,
                        });
                    }
                    items.push(first);
                    while matches!(self.peek(), Token::Comma) {
                        self.bump();
                        if matches!(self.peek(), Token::RBracket) {
                            break;
                        }
                        items.push(self.parse_full_expr()?);
                    }
                }
                self.expect(Token::RBracket, "expected `]`")?;
                Ok(Expr::List(items))
            }
            Token::LBrace => {
                if matches!(self.peek(), Token::RBrace) {
                    self.bump();
                    return Ok(Expr::Dict(vec![]));
                }

                let first = self.parse_full_expr()?;
                if matches!(self.peek(), Token::Colon) {
                    self.bump();
                    let first_value = self.parse_full_expr()?;
                    if matches!(self.peek(), Token::For) {
                        let clauses = self.parse_comprehension_clauses()?;
                        self.expect(Token::RBrace, "expected `}` after dict comprehension")?;
                        return Ok(Expr::DictComp {
                            key: Box::new(first),
                            value: Box::new(first_value),
                            clauses,
                        });
                    }
                    let mut pairs = vec![(first, first_value)];
                    while matches!(self.peek(), Token::Comma) {
                        self.bump();
                        if matches!(self.peek(), Token::RBrace) {
                            break;
                        }
                        let key = self.parse_full_expr()?;
                        self.expect(Token::Colon, "expected `:` in dict literal")?;
                        let value = self.parse_full_expr()?;
                        pairs.push((key, value));
                    }
                    self.expect(Token::RBrace, "expected `}`")?;
                    Ok(Expr::Dict(pairs))
                } else {
                    if matches!(self.peek(), Token::For) {
                        let clauses = self.parse_comprehension_clauses()?;
                        self.expect(Token::RBrace, "expected `}` after set comprehension")?;
                        return Ok(Expr::SetComp {
                            element: Box::new(first),
                            clauses,
                        });
                    }
                    let mut items = vec![first];
                    while matches!(self.peek(), Token::Comma) {
                        self.bump();
                        if matches!(self.peek(), Token::RBrace) {
                            break;
                        }
                        items.push(self.parse_full_expr()?);
                    }
                    self.expect(Token::RBrace, "expected `}`")?;
                    Ok(Expr::Set(items))
                }
            }
            token => Err(parse_err(
                self.line_no,
                &format!("unexpected token in expression: {token:?}"),
            )),
        }
    }

    fn parse_comprehension_clauses(&mut self) -> Result<Vec<ComprehensionClause>> {
        let mut clauses = Vec::new();
        while matches!(self.peek(), Token::For) {
            self.bump();
            let target = self.parse_comprehension_target()?;
            self.expect(Token::In, "expected `in` in comprehension")?;
            let iterable = self.parse_expr(0)?;
            let mut conditions = Vec::new();
            while matches!(self.peek(), Token::IfKw) {
                self.bump();
                conditions.push(self.parse_expr(0)?);
            }
            clauses.push(ComprehensionClause {
                target,
                iterable,
                conditions,
            });
        }
        if clauses.is_empty() {
            return Err(parse_err(self.line_no, "expected comprehension clause"));
        }
        Ok(clauses)
    }

    fn parse_comprehension_target(&mut self) -> Result<AssignTarget> {
        let first = self.parse_comprehension_target_atom()?;
        let mut items = vec![first];
        while matches!(self.peek(), Token::Comma) {
            self.bump();
            if matches!(self.peek(), Token::In) {
                break;
            }
            items.push(self.parse_comprehension_target_atom()?);
        }
        if items.len() == 1 {
            Ok(items.remove(0))
        } else {
            Ok(AssignTarget::Tuple(items))
        }
    }

    fn parse_comprehension_target_atom(&mut self) -> Result<AssignTarget> {
        match self.bump() {
            Token::Name(name) => Ok(AssignTarget::Name(name)),
            Token::LParen => {
                let target = self.parse_comprehension_target()?;
                self.expect(Token::RParen, "expected `)` in comprehension target")?;
                Ok(target)
            }
            token => Err(parse_err(
                self.line_no,
                &format!("invalid comprehension target token: {token:?}"),
            )),
        }
    }

    fn parse_argument_expr(&mut self) -> Result<Expr> {
        let expr = self.parse_full_expr()?;
        if matches!(self.peek(), Token::For) {
            let clauses = self.parse_comprehension_clauses()?;
            Ok(Expr::GeneratorComp {
                element: Box::new(expr),
                clauses,
            })
        } else {
            Ok(expr)
        }
    }

    fn parse_adjacent_string_literals(&mut self, first: Expr) -> Expr {
        let mut expr = first;
        loop {
            let next = match self.peek() {
                Token::Str(value) => Some(Expr::Str(value)),
                Token::FString(value) => Some(Expr::FString(value)),
                _ => None,
            };
            if let Some(next) = next {
                self.bump();
                expr = Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(expr),
                    right: Box::new(next),
                };
            } else {
                break;
            }
        }
        expr
    }

    fn parse_slice_tail(&mut self) -> Result<(Option<Expr>, Option<Expr>)> {
        let end = if matches!(self.peek(), Token::Colon | Token::RBracket) {
            None
        } else {
            Some(self.parse_full_expr()?)
        };
        let step = if matches!(self.peek(), Token::Colon) {
            self.bump();
            if matches!(self.peek(), Token::RBracket) {
                None
            } else {
                Some(self.parse_full_expr()?)
            }
        } else {
            None
        };
        Ok((end, step))
    }

    fn expect(&mut self, expected: Token, message: &str) -> Result<()> {
        let actual = self.bump();
        if actual == expected {
            Ok(())
        } else {
            Err(parse_err(
                self.line_no,
                &format!("{message}, got {actual:?}"),
            ))
        }
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        token
    }

    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof)
    }

    fn peek_n(&self, offset: usize) -> Token {
        self.tokens
            .get(self.pos + offset)
            .cloned()
            .unwrap_or(Token::Eof)
    }
}

fn parse_err(line_no: usize, message: &str) -> NanoPythonError {
    NanoPythonError::Parse(format!("line {line_no}: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_if_else_and_loop() {
        let src = r#"
count = 0
if count == 0:
    count = 1
else:
    count = 2
for i in range(0, 3):
    count = count + i
"#;
        let module = parse_source(src).expect("parse failed");
        assert_eq!(module.body.len(), 3);
    }

    #[test]
    fn parses_function_with_yield() {
        let src = r#"
def numbers(n):
    i = 0
    while i < n:
        yield i
        i = i + 1
"#;
        let module = parse_source(src).expect("parse failed");
        match &module.body[0] {
            Stmt::Def { is_generator, .. } => assert!(*is_generator),
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn supports_decorated_class() {
        let src = r#"
@dataclass
class Item:
    value = 1
"#;
        let module = parse_source(src).expect("parse failed");
        match &module.body[0] {
            Stmt::Class { decorators, .. } => assert_eq!(decorators.len(), 1),
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn parses_membership_and_elif() {
        let src = r#"
if x in values:
    y = 1
elif x is not None:
    y = 2
else:
    y = 3
"#;
        let module = parse_source(src).expect("parse failed");
        assert_eq!(module.body.len(), 1);
    }

    #[test]
    fn parses_join_with_list_comprehension() {
        let expr = parse_expression("\".\".join([msg.name for msg in lineage])", 1)
            .expect("expression parse failed");
        match expr {
            Expr::Call { .. } => {}
            other => panic!("unexpected expr: {other:?}"),
        }
    }

    #[test]
    #[ignore]
    fn parses_fory_compiler_sources() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fory-main/compiler/fory_compiler");
        let mut entries = Vec::new();
        collect_py_files(&root, &mut entries);
        entries.sort();

        for path in entries {
            let source = std::fs::read_to_string(&path).expect("read file");
            if let Err(err) = parse_source(&source) {
                let mut detail = String::new();
                if let Some(line_no) = extract_line_no(&err.to_string()) {
                    if let Some(line) = source.lines().nth(line_no.saturating_sub(1)) {
                        detail = format!("\nline {line_no}: {line}");
                    }
                }
                panic!("failed on {}: {err}{detail}", path.display());
            }
        }
    }

    fn extract_line_no(message: &str) -> Option<usize> {
        let marker = "line ";
        let start = message.find(marker)? + marker.len();
        let rest = &message[start..];
        let end = rest.find(':')?;
        rest[..end].trim().parse().ok()
    }

    fn collect_py_files(root: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(root).expect("read_dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) == Some("tests") {
                    continue;
                }
                collect_py_files(&path, out);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("py") {
                out.push(path);
            }
        }
    }
}
