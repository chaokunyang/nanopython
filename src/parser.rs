use crate::ast::{BinaryOp, ExceptHandler, Expr, ImportItem, Param, ParamKind, Program, Stmt, UnaryOp};
use crate::token::{Keyword, Token, TokenKind};

type ParseResult<T> = Result<T, String>;

pub fn parse(tokens: Vec<Token>) -> ParseResult<Program> {
    let mut parser = Parser { tokens, pos: 0 };
    parser.parse_program()
}

pub fn parse_expression(tokens: Vec<Token>) -> ParseResult<Expr> {
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.parse_expr_with_commas()?;
    parser.skip_newlines();
    if parser.at_eof() {
        Ok(expr)
    } else {
        let tok = parser.current();
        Err(format!(
            "unexpected token {:?} at {}:{}",
            tok.kind, tok.line, tok.col
        ))
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn parse_program(&mut self) -> ParseResult<Program> {
        let mut body = Vec::new();
        self.skip_newlines();
        while !self.at_eof() {
            body.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        Ok(Program { body })
    }

    fn parse_stmt(&mut self) -> ParseResult<Stmt> {
        if self.at(TokenKind::At) {
            return self.parse_decorated_stmt();
        }
        if self.at_keyword(Keyword::If) {
            return self.parse_if();
        }
        if self.at_keyword(Keyword::While) {
            return self.parse_while();
        }
        if self.at_keyword(Keyword::For) {
            return self.parse_for();
        }
        if self.at_keyword(Keyword::Def) {
            return self.parse_function_def();
        }
        if self.at_keyword(Keyword::Class) {
            return self.parse_class_def();
        }
        if self.at_keyword(Keyword::Return) {
            return self.parse_return();
        }
        if self.at_keyword(Keyword::Raise) {
            return self.parse_raise();
        }
        if self.at_keyword(Keyword::Try) {
            return self.parse_try();
        }
        if self.at_keyword(Keyword::Break) {
            self.advance();
            self.expect_newline()?;
            return Ok(Stmt::Break);
        }
        if self.at_keyword(Keyword::Continue) {
            self.advance();
            self.expect_newline()?;
            return Ok(Stmt::Continue);
        }
        if self.at_keyword(Keyword::Pass) {
            self.advance();
            self.expect_newline()?;
            return Ok(Stmt::Pass);
        }
        if self.at_keyword(Keyword::Import) {
            return self.parse_import();
        }
        if self.at_keyword(Keyword::From) {
            return self.parse_from_import();
        }
        if self.at_keyword(Keyword::With) {
            return self.parse_with();
        }
        self.parse_simple_stmt()
    }

    fn parse_decorated_stmt(&mut self) -> ParseResult<Stmt> {
        let mut decorators = Vec::new();
        while self.at(TokenKind::At) {
            self.advance();
            decorators.push(self.parse_expr()?);
            self.expect_newline()?;
        }
        if self.at_keyword(Keyword::Def) {
            return self.parse_function_def_with_decorators(decorators);
        }
        if self.at_keyword(Keyword::Class) {
            return self.parse_class_def_with_decorators(decorators);
        }
        Err("decorator must be followed by function or class definition".to_owned())
    }

    fn parse_if(&mut self) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::If)?;
        let test = self.parse_expr()?;
        let body = self.parse_suite()?;
        let mut orelse = Vec::new();

        if self.at_keyword(Keyword::Elif) {
            orelse.push(self.parse_if_elif()?);
        } else if self.at_keyword(Keyword::Else) {
            self.advance();
            orelse = self.parse_suite()?;
        }
        Ok(Stmt::If { test, body, orelse })
    }

    fn parse_if_elif(&mut self) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::Elif)?;
        let test = self.parse_expr()?;
        let body = self.parse_suite()?;
        let mut orelse = Vec::new();
        if self.at_keyword(Keyword::Elif) {
            orelse.push(self.parse_if_elif()?);
        } else if self.at_keyword(Keyword::Else) {
            self.advance();
            orelse = self.parse_suite()?;
        }
        Ok(Stmt::If { test, body, orelse })
    }

    fn parse_while(&mut self) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::While)?;
        let test = self.parse_expr()?;
        let body = self.parse_suite()?;
        Ok(Stmt::While { test, body })
    }

    fn parse_for(&mut self) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::For)?;
        let target = self.parse_for_target_expr()?;
        self.expect_keyword(Keyword::In)?;
        let iter = self.parse_expr()?;
        let body = self.parse_suite()?;
        Ok(Stmt::For { target, iter, body })
    }

    fn parse_function_def(&mut self) -> ParseResult<Stmt> {
        self.parse_function_def_with_decorators(Vec::new())
    }

    fn parse_function_def_with_decorators(&mut self, decorators: Vec<Expr>) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::Def)?;
        let name = self.expect_name()?;
        self.expect(TokenKind::LParen)?;
        self.skip_newlines();
        let mut params = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                self.skip_newlines();
                if self.at(TokenKind::RParen) {
                    break;
                }
                let (param_name, kind) = if self.at(TokenKind::Star) {
                    self.advance();
                    if self.at(TokenKind::Star) {
                        self.advance();
                        (self.expect_name()?, ParamKind::VarKwargs)
                    } else if self.at(TokenKind::Comma) || self.at(TokenKind::RParen) {
                        if self.at(TokenKind::Comma) {
                            self.advance();
                        }
                        self.skip_newlines();
                        continue;
                    } else {
                        (self.expect_name()?, ParamKind::VarArgs)
                    }
                } else {
                    (self.expect_name()?, ParamKind::Positional)
                };
                if self.at(TokenKind::Colon) {
                    self.advance();
                    self.skip_annotation_in_param();
                }
                let default = if self.at(TokenKind::Eq) {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                params.push(Param {
                    name: param_name,
                    default,
                    kind,
                });
                if self.at(TokenKind::Comma) {
                    self.advance();
                    self.skip_newlines();
                    if self.at(TokenKind::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.skip_newlines();
        self.expect(TokenKind::RParen)?;
        if self.at(TokenKind::Minus) && matches!(self.peek_kind(1), Some(TokenKind::Gt)) {
            self.advance();
            self.advance();
            self.skip_annotation_to_colon();
        }
        let body = self.parse_suite()?;
        Ok(Stmt::FunctionDef {
            name,
            decorators,
            params,
            body,
        })
    }

    fn parse_class_def(&mut self) -> ParseResult<Stmt> {
        self.parse_class_def_with_decorators(Vec::new())
    }

    fn parse_class_def_with_decorators(&mut self, decorators: Vec<Expr>) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::Class)?;
        let name = self.expect_name()?;
        let mut bases = Vec::new();
        if self.at(TokenKind::LParen) {
            self.advance();
            if !self.at(TokenKind::RParen) {
                loop {
                    bases.push(self.parse_expr()?);
                    if self.at(TokenKind::Comma) {
                        self.advance();
                        if self.at(TokenKind::RParen) {
                            break;
                        }
                        continue;
                    }
                    break;
                }
            }
            self.expect(TokenKind::RParen)?;
        }
        let body = self.parse_suite()?;
        Ok(Stmt::ClassDef {
            name,
            decorators,
            bases,
            body,
        })
    }

    fn parse_return(&mut self) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::Return)?;
        if self.at(TokenKind::Newline) {
            self.advance();
            return Ok(Stmt::Return(None));
        }
        let expr = self.parse_expr_with_commas()?;
        self.expect_newline()?;
        Ok(Stmt::Return(Some(expr)))
    }

    fn parse_raise(&mut self) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::Raise)?;
        if self.at(TokenKind::Newline) {
            self.advance();
            return Ok(Stmt::Raise(None));
        }
        let expr = self.parse_expr()?;
        if self.at_keyword(Keyword::From) {
            self.advance();
            let _ = self.parse_expr()?;
        }
        self.expect_newline()?;
        Ok(Stmt::Raise(Some(expr)))
    }

    fn parse_try(&mut self) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::Try)?;
        let body = self.parse_suite()?;

        let mut handlers = Vec::new();
        while self.at_keyword(Keyword::Except) {
            self.advance();
            let exc_name = if self.at(TokenKind::Colon) {
                None
            } else {
                self.skip_except_type();
                if self.at_keyword(Keyword::As) {
                    self.advance();
                    Some(self.expect_name()?)
                } else {
                    None
                }
            };
            let handler_body = self.parse_suite()?;
            handlers.push(ExceptHandler {
                exc_name,
                body: handler_body,
            });
        }

        let mut orelse = Vec::new();
        if self.at_keyword(Keyword::Else) {
            self.advance();
            orelse = self.parse_suite()?;
        }

        let mut finalbody = Vec::new();
        if self.at_keyword(Keyword::Finally) {
            self.advance();
            finalbody = self.parse_suite()?;
        }

        if handlers.is_empty() && finalbody.is_empty() {
            return Err("try statement requires except or finally block".to_owned());
        }
        Ok(Stmt::Try {
            body,
            handlers,
            orelse,
            finalbody,
        })
    }

    fn parse_import(&mut self) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::Import)?;
        let mut names = Vec::new();
        loop {
            names.push(self.parse_dotted_name()?);
            if self.at(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect_newline()?;
        Ok(Stmt::Import(names))
    }

    fn parse_from_import(&mut self) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::From)?;
        let module = self.parse_from_module_name()?;
        self.expect_keyword(Keyword::Import)?;
        let mut names = Vec::new();
        let parenthesized = if self.at(TokenKind::LParen) {
            self.advance();
            self.skip_newlines();
            true
        } else {
            false
        };
        loop {
            if parenthesized && self.at(TokenKind::RParen) {
                self.advance();
                break;
            }
            if self.at(TokenKind::Star) {
                self.advance();
                names.push(ImportItem {
                    name: "*".to_owned(),
                    asname: None,
                });
                if parenthesized {
                    self.skip_newlines();
                    self.expect(TokenKind::RParen)?;
                }
                break;
            }
            let name = self.expect_name()?;
            let asname = if self.at_keyword(Keyword::As) {
                self.advance();
                Some(self.expect_name()?)
            } else {
                None
            };
            names.push(ImportItem { name, asname });
            if self.at(TokenKind::Comma) {
                self.advance();
                if parenthesized {
                    self.skip_newlines();
                }
                if parenthesized && self.at(TokenKind::RParen) {
                    self.advance();
                    break;
                }
            } else {
                break;
            }
        }
        self.expect_newline()?;
        Ok(Stmt::FromImport { module, names })
    }

    fn parse_with(&mut self) -> ParseResult<Stmt> {
        self.expect_keyword(Keyword::With)?;
        let context = self.parse_expr()?;
        let asname = if self.at_keyword(Keyword::As) {
            self.advance();
            Some(self.expect_name()?)
        } else {
            None
        };
        let body = self.parse_suite()?;
        Ok(Stmt::With {
            context,
            asname,
            body,
        })
    }

    fn parse_simple_stmt(&mut self) -> ParseResult<Stmt> {
        let expr = self.parse_expr_with_commas()?;

        if self.at(TokenKind::Colon) {
            self.advance();
            self.skip_annotation_in_annassign();
            let value = if self.at(TokenKind::Eq) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect_newline()?;
            return Ok(Stmt::AnnAssign {
                target: expr,
                value,
            });
        }
        if self.at(TokenKind::Eq) {
            self.advance();
            let value = self.parse_expr()?;
            self.expect_newline()?;
            Ok(Stmt::Assign {
                target: expr,
                value,
            })
        } else if let Some(op) = self.current_augassign_op() {
            self.advance();
            let value = self.parse_expr()?;
            self.expect_newline()?;
            Ok(Stmt::AugAssign {
                target: expr,
                op,
                value,
            })
        } else {
            self.expect_newline()?;
            Ok(Stmt::Expr(expr))
        }
    }

    fn parse_suite(&mut self) -> ParseResult<Vec<Stmt>> {
        self.expect(TokenKind::Colon)?;
        self.expect_newline()?;
        self.expect(TokenKind::Indent)?;
        let mut body = Vec::new();
        self.skip_newlines();
        while !self.at(TokenKind::Dedent) && !self.at_eof() {
            body.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;
        Ok(body)
    }

    fn parse_dotted_name(&mut self) -> ParseResult<String> {
        let mut name = self.expect_name()?;
        while self.at(TokenKind::Dot) {
            self.advance();
            name.push('.');
            name.push_str(&self.expect_name()?);
        }
        Ok(name)
    }

    fn parse_from_module_name(&mut self) -> ParseResult<String> {
        let mut module = String::new();
        while self.at(TokenKind::Dot) {
            self.advance();
            module.push('.');
        }
        if matches!(self.current().kind, TokenKind::Name(_)) {
            module.push_str(&self.expect_name()?);
            while self.at(TokenKind::Dot) {
                self.advance();
                module.push('.');
                module.push_str(&self.expect_name()?);
            }
        }
        if module.is_empty() {
            let tok = self.current();
            return Err(format!(
                "expected module name, found {:?} at {}:{}",
                tok.kind, tok.line, tok.col
            ));
        }
        Ok(module)
    }

    fn parse_expr(&mut self) -> ParseResult<Expr> {
        if self.at_keyword(Keyword::Lambda) {
            return self.parse_lambda_expr();
        }
        self.parse_conditional_expr()
    }

    fn parse_lambda_expr(&mut self) -> ParseResult<Expr> {
        self.expect_keyword(Keyword::Lambda)?;
        let mut params = Vec::new();
        if !self.at(TokenKind::Colon) {
            loop {
                let (name, kind) = if self.at(TokenKind::Star) {
                    self.advance();
                    if self.at(TokenKind::Star) {
                        self.advance();
                        (self.expect_name()?, ParamKind::VarKwargs)
                    } else {
                        (self.expect_name()?, ParamKind::VarArgs)
                    }
                } else {
                    (self.expect_name()?, ParamKind::Positional)
                };
                let default = if self.at(TokenKind::Eq) {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                params.push(Param {
                    name,
                    default,
                    kind,
                });
                if self.at(TokenKind::Comma) {
                    self.advance();
                    if self.at(TokenKind::Colon) {
                        break;
                    }
                    continue;
                }
                break;
            }
        }
        self.expect(TokenKind::Colon)?;
        let body = self.parse_expr()?;
        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
        })
    }

    fn parse_expr_with_commas(&mut self) -> ParseResult<Expr> {
        let mut items = vec![self.parse_expr()?];
        let mut had_comma = false;
        while self.at(TokenKind::Comma) {
            had_comma = true;
            self.advance();
            if self.at(TokenKind::RParen)
                || self.at(TokenKind::RBracket)
                || self.at(TokenKind::RBrace)
                || self.at(TokenKind::Newline)
            {
                break;
            }
            items.push(self.parse_expr()?);
        }
        if items.len() == 1 && !had_comma {
            Ok(items.pop().expect("one item expected"))
        } else {
            Ok(Expr::List(items))
        }
    }

    fn parse_conditional_expr(&mut self) -> ParseResult<Expr> {
        let then_expr = self.parse_or()?;
        if self.at_keyword(Keyword::If) {
            self.advance();
            let condition = self.parse_or()?;
            self.expect_keyword(Keyword::Else)?;
            let else_expr = self.parse_conditional_expr()?;
            Ok(Expr::IfExpr {
                then_expr: Box::new(then_expr),
                condition: Box::new(condition),
                else_expr: Box::new(else_expr),
            })
        } else {
            Ok(then_expr)
        }
    }

    fn parse_or(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_and()?;
        while self.at_keyword(Keyword::Or) {
            self.advance();
            let right = self.parse_and()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_compare()?;
        while self.at_keyword(Keyword::And) {
            self.advance();
            let right = self.parse_compare()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_compare(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_bitor()?;
        loop {
            let (op, consume_tokens) = if self.at(TokenKind::EqEq) {
                (Some(BinaryOp::Eq), 1)
            } else if self.at(TokenKind::NotEq) {
                (Some(BinaryOp::Ne), 1)
            } else if self.at(TokenKind::Lt) {
                (Some(BinaryOp::Lt), 1)
            } else if self.at(TokenKind::Lte) {
                (Some(BinaryOp::Lte), 1)
            } else if self.at(TokenKind::Gt) {
                (Some(BinaryOp::Gt), 1)
            } else if self.at(TokenKind::Gte) {
                (Some(BinaryOp::Gte), 1)
            } else if self.at_keyword(Keyword::In) {
                (Some(BinaryOp::In), 1)
            } else if self.at_keyword(Keyword::Not)
                && matches!(self.peek_kind(1), Some(TokenKind::Keyword(Keyword::In)))
            {
                (Some(BinaryOp::NotIn), 2)
            } else if self.at_keyword(Keyword::Is)
                && matches!(self.peek_kind(1), Some(TokenKind::Keyword(Keyword::Not)))
            {
                (Some(BinaryOp::Ne), 2)
            } else if self.at_keyword(Keyword::Is) {
                (Some(BinaryOp::Eq), 1)
            } else {
                (None, 0)
            };
            if let Some(op) = op {
                for _ in 0..consume_tokens {
                    self.advance();
                }
                let right = self.parse_bitor()?;
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_bitor(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_bitxor()?;
        while self.at(TokenKind::Pipe) {
            self.advance();
            let right = self.parse_bitxor()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitOr,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_bitxor(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_bitand()?;
        while self.at(TokenKind::Caret) {
            self.advance();
            let right = self.parse_bitand()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitXor,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_bitand(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_shift()?;
        while self.at(TokenKind::Amp) {
            self.advance();
            let right = self.parse_shift()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitAnd,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_shift(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_add()?;
        loop {
            let op = if self.at(TokenKind::LShift) {
                Some(BinaryOp::LShift)
            } else if self.at(TokenKind::RShift) {
                Some(BinaryOp::RShift)
            } else {
                None
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_add()?;
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_add(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_mul()?;
        loop {
            let op = if self.at(TokenKind::Plus) {
                Some(BinaryOp::Add)
            } else if self.at(TokenKind::Minus) {
                Some(BinaryOp::Sub)
            } else {
                None
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_mul()?;
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_mul(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_unary()?;
        loop {
            let op = if self.at(TokenKind::Star) {
                Some(BinaryOp::Mul)
            } else if self.at(TokenKind::Slash) {
                Some(BinaryOp::Div)
            } else if self.at(TokenKind::SlashSlash) {
                Some(BinaryOp::Div)
            } else if self.at(TokenKind::Percent) {
                Some(BinaryOp::Mod)
            } else {
                None
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_unary()?;
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> ParseResult<Expr> {
        if self.at(TokenKind::Star) {
            self.advance();
            let expr = self.parse_unary()?;
            return Ok(Expr::Starred(Box::new(expr)));
        }
        if self.at(TokenKind::Minus) {
            self.advance();
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            });
        }
        if self.at_keyword(Keyword::Not) {
            self.advance();
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_atom()?;
        loop {
            if self.at(TokenKind::LParen) {
                self.advance();
                let mut args = Vec::new();
                let mut kwargs = Vec::new();
                if !self.at(TokenKind::RParen) {
                    loop {
                        if let Some(name) = self.lookahead_kwarg_name() {
                            self.advance(); // name
                            self.expect(TokenKind::Eq)?;
                            let value = self.parse_expr()?;
                            kwargs.push((name, value));
                        } else {
                            let mut arg_expr = self.parse_expr()?;
                            if self.at_keyword(Keyword::For) {
                                let (target, iter, cond) = self.parse_comprehension_tail()?;
                                arg_expr = Expr::GenComp {
                                    elem: Box::new(arg_expr),
                                    target: Box::new(target),
                                    iter: Box::new(iter),
                                    cond: cond.map(Box::new),
                                };
                            }
                            args.push(arg_expr);
                        }
                        if self.at(TokenKind::Comma) {
                            self.advance();
                            if self.at(TokenKind::RParen) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RParen)?;
                expr = Expr::Call {
                    func: Box::new(expr),
                    args,
                    kwargs,
                };
                continue;
            }
            if self.at(TokenKind::Dot) {
                self.advance();
                let name = self.expect_name()?;
                expr = Expr::Attr {
                    value: Box::new(expr),
                    name,
                };
                continue;
            }
            if self.at(TokenKind::LBracket) {
                self.advance();
                let index = if self.at(TokenKind::Colon) {
                    self.advance();
                    let stop = if !self.at(TokenKind::RBracket) {
                        Some(Box::new(self.parse_expr_with_commas()?))
                    } else {
                        None
                    };
                    Expr::Slice { start: None, stop }
                } else {
                    let first = self.parse_expr_with_commas()?;
                    if self.at(TokenKind::Colon) {
                        self.advance();
                        let stop = if !self.at(TokenKind::RBracket) {
                            Some(Box::new(self.parse_expr_with_commas()?))
                        } else {
                            None
                        };
                        Expr::Slice {
                            start: Some(Box::new(first)),
                            stop,
                        }
                    } else {
                        first
                    }
                };
                self.expect(TokenKind::RBracket)?;
                expr = Expr::Subscript {
                    value: Box::new(expr),
                    index: Box::new(index),
                };
                continue;
            }
            break;
        }
        Ok(expr)
    }

    fn parse_atom(&mut self) -> ParseResult<Expr> {
        if self.at_keyword(Keyword::Yield) {
            self.advance();
            if self.at(TokenKind::Newline)
                || self.at(TokenKind::Comma)
                || self.at(TokenKind::RParen)
                || self.at(TokenKind::RBracket)
            {
                return Ok(Expr::Yield(None));
            }
            let value = self.parse_expr()?;
            return Ok(Expr::Yield(Some(Box::new(value))));
        }

        let token = self.current().clone();
        match token.kind {
            TokenKind::Name(name) => {
                self.advance();
                Ok(Expr::Name(name))
            }
            TokenKind::Int(v) => {
                self.advance();
                Ok(Expr::Int(v))
            }
            TokenKind::Str(s) => {
                self.advance();
                let mut parts = vec![Expr::Str(s)];
                loop {
                    let next = self.current().clone();
                    match next.kind {
                        TokenKind::Str(part) => {
                            self.advance();
                            parts.push(Expr::Str(part));
                        }
                        TokenKind::FStr(part) => {
                            self.advance();
                            parts.push(Expr::FStr(part));
                        }
                        _ => break,
                    }
                }
                Ok(self.concat_string_parts(parts))
            }
            TokenKind::FStr(s) => {
                self.advance();
                let mut parts = vec![Expr::FStr(s)];
                loop {
                    let next = self.current().clone();
                    match next.kind {
                        TokenKind::Str(part) => {
                            self.advance();
                            parts.push(Expr::Str(part));
                        }
                        TokenKind::FStr(part) => {
                            self.advance();
                            parts.push(Expr::FStr(part));
                        }
                        _ => break,
                    }
                }
                Ok(self.concat_string_parts(parts))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            TokenKind::Keyword(Keyword::None) => {
                self.advance();
                Ok(Expr::None)
            }
            TokenKind::LParen => {
                self.advance();
                if self.at(TokenKind::RParen) {
                    self.advance();
                    return Ok(Expr::List(Vec::new()));
                }
                let expr = self.parse_expr_with_commas()?;
                if self.at_keyword(Keyword::For) {
                    let (target, iter, cond) = self.parse_comprehension_tail()?;
                    self.expect(TokenKind::RParen)?;
                    return Ok(Expr::GenComp {
                        elem: Box::new(expr),
                        target: Box::new(target),
                        iter: Box::new(iter),
                        cond: cond.map(Box::new),
                    });
                }
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            TokenKind::LBracket => self.parse_list(),
            TokenKind::LBrace => self.parse_braced_literal(),
            _ => Err(format!(
                "unexpected token {:?} at {}:{}",
                token.kind, token.line, token.col
            )),
        }
    }

    fn concat_string_parts(&self, mut parts: Vec<Expr>) -> Expr {
        if parts.len() == 1 {
            return parts.pop().expect("one string part expected");
        }
        let mut expr = parts.remove(0);
        for part in parts {
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Add,
                right: Box::new(part),
            };
        }
        expr
    }

    fn parse_list(&mut self) -> ParseResult<Expr> {
        self.expect(TokenKind::LBracket)?;
        if self.at(TokenKind::RBracket) {
            self.advance();
            return Ok(Expr::List(Vec::new()));
        }
        let first = self.parse_expr()?;
        if self.at_keyword(Keyword::For) {
            let (target, iter, cond) = self.parse_comprehension_tail()?;
            self.expect(TokenKind::RBracket)?;
            return Ok(Expr::ListComp {
                elem: Box::new(first),
                target: Box::new(target),
                iter: Box::new(iter),
                cond: cond.map(Box::new),
            });
        }
        let mut values = vec![first];
        while self.at(TokenKind::Comma) {
            self.advance();
            if self.at(TokenKind::RBracket) {
                break;
            }
            values.push(self.parse_expr()?);
        }
        self.expect(TokenKind::RBracket)?;
        Ok(Expr::List(values))
    }

    fn parse_braced_literal(&mut self) -> ParseResult<Expr> {
        self.expect(TokenKind::LBrace)?;
        if self.at(TokenKind::RBrace) {
            self.advance();
            return Ok(Expr::Dict(Vec::new()));
        }

        let first = self.parse_expr()?;
        if self.at(TokenKind::Colon) {
            self.advance();
            let first_value = self.parse_expr()?;
            if self.at_keyword(Keyword::For) {
                let (target, iter, cond) = self.parse_comprehension_tail()?;
                self.expect(TokenKind::RBrace)?;
                return Ok(Expr::DictComp {
                    key: Box::new(first),
                    value: Box::new(first_value),
                    target: Box::new(target),
                    iter: Box::new(iter),
                    cond: cond.map(Box::new),
                });
            }
            let mut items = vec![(first, first_value)];
            while self.at(TokenKind::Comma) {
                self.advance();
                if self.at(TokenKind::RBrace) {
                    break;
                }
                let key = self.parse_expr()?;
                self.expect(TokenKind::Colon)?;
                let value = self.parse_expr()?;
                items.push((key, value));
            }
            self.expect(TokenKind::RBrace)?;
            Ok(Expr::Dict(items))
        } else {
            if self.at_keyword(Keyword::For) {
                let (target, iter, cond) = self.parse_comprehension_tail()?;
                self.expect(TokenKind::RBrace)?;
                return Ok(Expr::SetComp {
                    elem: Box::new(first),
                    target: Box::new(target),
                    iter: Box::new(iter),
                    cond: cond.map(Box::new),
                });
            }
            let mut items = vec![first];
            while self.at(TokenKind::Comma) {
                self.advance();
                if self.at(TokenKind::RBrace) {
                    break;
                }
                items.push(self.parse_expr()?);
            }
            self.expect(TokenKind::RBrace)?;
            Ok(Expr::Set(items))
        }
    }

    fn lookahead_kwarg_name(&self) -> Option<String> {
        let current = self.tokens.get(self.pos)?;
        let next = self.tokens.get(self.pos + 1)?;
        match (&current.kind, &next.kind) {
            (TokenKind::Name(name), TokenKind::Eq) => Some(name.clone()),
            _ => None,
        }
    }

    fn parse_for_target_expr(&mut self) -> ParseResult<Expr> {
        let mut targets = Vec::new();
        targets.push(self.parse_for_target_item()?);
        while self.at(TokenKind::Comma) {
            self.advance();
            if self.at_keyword(Keyword::In) {
                break;
            }
            targets.push(self.parse_for_target_item()?);
        }
        if targets.len() == 1 {
            targets
                .pop()
                .ok_or_else(|| "invalid comprehension target".to_owned())
        } else {
            Ok(Expr::List(targets))
        }
    }

    fn parse_for_target_item(&mut self) -> ParseResult<Expr> {
        if self.at(TokenKind::LParen) || self.at(TokenKind::LBracket) {
            let close = if self.at(TokenKind::LParen) {
                TokenKind::RParen
            } else {
                TokenKind::RBracket
            };
            self.advance();
            let mut items = Vec::new();
            let mut had_comma = false;
            if !self.at(close.clone()) {
                loop {
                    let item = if self.at(TokenKind::Star) {
                        self.advance();
                        Expr::Starred(Box::new(self.parse_for_target_item()?))
                    } else {
                        self.parse_for_target_item()?
                    };
                    items.push(item);
                    if self.at(TokenKind::Comma) {
                        had_comma = true;
                        self.advance();
                        if self.at(close.clone()) {
                            break;
                        }
                        continue;
                    }
                    break;
                }
            }
            self.expect(close)?;
            if items.len() == 1 && !had_comma {
                return items
                    .pop()
                    .ok_or_else(|| "invalid target grouping".to_owned());
            }
            return Ok(Expr::List(items));
        }
        let mut target = Expr::Name(self.expect_name()?);
        loop {
            if self.at(TokenKind::Dot) {
                self.advance();
                let name = self.expect_name()?;
                target = Expr::Attr {
                    value: Box::new(target),
                    name,
                };
                continue;
            }
            if self.at(TokenKind::LBracket) {
                self.advance();
                let index = if self.at(TokenKind::Colon) {
                    self.advance();
                    let stop = if !self.at(TokenKind::RBracket) {
                        Some(Box::new(self.parse_expr_with_commas()?))
                    } else {
                        None
                    };
                    Expr::Slice { start: None, stop }
                } else {
                    let first = self.parse_expr_with_commas()?;
                    if self.at(TokenKind::Colon) {
                        self.advance();
                        let stop = if !self.at(TokenKind::RBracket) {
                            Some(Box::new(self.parse_expr_with_commas()?))
                        } else {
                            None
                        };
                        Expr::Slice {
                            start: Some(Box::new(first)),
                            stop,
                        }
                    } else {
                        first
                    }
                };
                self.expect(TokenKind::RBracket)?;
                target = Expr::Subscript {
                    value: Box::new(target),
                    index: Box::new(index),
                };
                continue;
            }
            break;
        }
        Ok(target)
    }

    fn parse_comprehension_tail(&mut self) -> ParseResult<(Expr, Expr, Option<Expr>)> {
        self.expect_keyword(Keyword::For)?;
        let target = self.parse_for_target_expr()?;
        self.expect_keyword(Keyword::In)?;
        let iter = self.parse_or()?;
        let cond = if self.at_keyword(Keyword::If) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok((target, iter, cond))
    }

    fn skip_annotation_in_param(&mut self) {
        let mut depth = 0isize;
        loop {
            let kind = &self.current().kind;
            if depth == 0
                && matches!(
                    kind,
                    TokenKind::Eq | TokenKind::Comma | TokenKind::RParen | TokenKind::Newline
                )
            {
                break;
            }
            match kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                TokenKind::Eof => break,
                _ => {}
            }
            self.advance();
        }
    }

    fn skip_annotation_to_colon(&mut self) {
        let mut depth = 0isize;
        loop {
            let kind = &self.current().kind;
            if depth == 0 && matches!(kind, TokenKind::Colon | TokenKind::Newline | TokenKind::Eof)
            {
                break;
            }
            match kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                _ => {}
            }
            self.advance();
        }
    }

    fn skip_annotation_in_annassign(&mut self) {
        let mut depth = 0isize;
        loop {
            let kind = &self.current().kind;
            if depth == 0 && matches!(kind, TokenKind::Eq | TokenKind::Newline | TokenKind::Eof) {
                break;
            }
            match kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                _ => {}
            }
            self.advance();
        }
    }

    fn skip_except_type(&mut self) {
        let mut depth = 0isize;
        loop {
            let kind = &self.current().kind;
            if depth == 0
                && (matches!(kind, TokenKind::Colon | TokenKind::Eof)
                    || matches!(kind, TokenKind::Keyword(Keyword::As)))
            {
                break;
            }
            match kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                _ => {}
            }
            self.advance();
        }
    }

    fn skip_newlines(&mut self) {
        while self.at(TokenKind::Newline) {
            self.advance();
        }
    }

    fn expect_newline(&mut self) -> ParseResult<()> {
        if self.at(TokenKind::Newline) {
            self.advance();
            Ok(())
        } else {
            let tok = self.current();
            Err(format!(
                "expected newline, found {:?} at {}:{}",
                tok.kind, tok.line, tok.col
            ))
        }
    }

    fn expect_name(&mut self) -> ParseResult<String> {
        let tok = self.current().clone();
        match tok.kind {
            TokenKind::Name(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(format!(
                "expected name, found {:?} at {}:{}",
                tok.kind, tok.line, tok.col
            )),
        }
    }

    fn expect_keyword(&mut self, kw: Keyword) -> ParseResult<()> {
        if self.at_keyword(kw) {
            self.advance();
            Ok(())
        } else {
            let tok = self.current();
            Err(format!(
                "expected keyword {:?}, found {:?} at {}:{}",
                kw, tok.kind, tok.line, tok.col
            ))
        }
    }

    fn expect(&mut self, expected: TokenKind) -> ParseResult<()> {
        if self.at(expected.clone()) {
            self.advance();
            Ok(())
        } else {
            let tok = self.current();
            Err(format!(
                "expected {:?}, found {:?} at {}:{}",
                expected, tok.kind, tok.line, tok.col
            ))
        }
    }

    fn at_keyword(&self, kw: Keyword) -> bool {
        matches!(self.current().kind, TokenKind::Keyword(k) if k == kw)
    }

    fn current_augassign_op(&self) -> Option<BinaryOp> {
        match self.current().kind {
            TokenKind::PlusEq => Some(BinaryOp::Add),
            TokenKind::MinusEq => Some(BinaryOp::Sub),
            TokenKind::StarEq => Some(BinaryOp::Mul),
            TokenKind::SlashEq => Some(BinaryOp::Div),
            TokenKind::SlashSlashEq => Some(BinaryOp::Div),
            TokenKind::PercentEq => Some(BinaryOp::Mod),
            TokenKind::AmpEq => Some(BinaryOp::BitAnd),
            TokenKind::PipeEq => Some(BinaryOp::BitOr),
            TokenKind::CaretEq => Some(BinaryOp::BitXor),
            TokenKind::LShiftEq => Some(BinaryOp::LShift),
            TokenKind::RShiftEq => Some(BinaryOp::RShift),
            _ => None,
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn at_eof(&self) -> bool {
        matches!(self.current().kind, TokenKind::Eof)
    }

    fn current(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .unwrap_or_else(|| self.tokens.last().expect("token stream cannot be empty"))
    }

    fn peek_kind(&self, offset: usize) -> Option<&TokenKind> {
        self.tokens.get(self.pos + offset).map(|t| &t.kind)
    }

    fn advance(&mut self) {
        if self.pos < self.tokens.len().saturating_sub(1) {
            self.pos += 1;
        }
    }
}
