
use crate::lexer::{Lexer, Token};
use crate::ast::{Program, Statement, Expression};
use bumpalo::Bump;

#[derive(PartialEq, PartialOrd)]
enum Precedence {
    Lowest,
    Equals,      // ==
    LessGreater, // > or <
    Sum,         // +
    Product,     // *
    Prefix,      // -X or !X
    Call,        // myFunction(X)
    Index,       // array[index]
    Member,      // obj.field
}

pub struct Parser<'src, 'arena> {
    lexer: Lexer<'src>,
    pub arena: &'arena Bump,
    cur_token: Token,
    peek_token: Token,
    pub errors: Vec<String>,
}

impl<'src, 'arena> Parser<'src, 'arena> {
    pub fn new(lexer: Lexer<'src>, arena: &'arena Bump) -> Self {
        let mut p = Parser {
            lexer,
            arena,
            cur_token: Token::EOF,
            peek_token: Token::EOF,
            errors: vec![],
        };
        p.next_token();
        p.next_token();
        p
    }

    pub fn next_token(&mut self) {
        self.cur_token = self.peek_token.clone();
        self.peek_token = self.lexer.next_token();
    }

    pub fn parse_program(&mut self) -> Program<'arena> {
        let mut program = Program::new();

        while self.cur_token != Token::EOF {
            if let Some(stmt) = self.parse_statement() {
                program.statements.push(stmt);
            }
            self.next_token();
        }
        program
    }

    fn parse_statement(&mut self) -> Option<Statement<'arena>> {
        match self.cur_token {
            Token::Let => self.parse_let_statement(false),
            Token::Mut => self.parse_let_statement(true),
            Token::Fn => self.parse_fn_statement(),
            Token::Return => self.parse_return_statement(),
            Token::While => self.parse_while_statement(),
            Token::For => self.parse_for_statement(),
            Token::Class => self.parse_class_statement(),
            Token::Struct => self.parse_struct_statement(),
            Token::Enum => self.parse_enum_statement(),
            Token::Trait => self.parse_trait_statement(),
            Token::Impl => self.parse_impl_statement(),
            Token::Hash => self.parse_hash_directive(),
            Token::Break => Some(Statement::Break { line: self.lexer.line }),
            Token::Continue => Some(Statement::Continue { line: self.lexer.line }),
            _ => self.parse_expression_statement(),
        }
    }

    // --- Hash Directives (#include, #version) ---
    fn parse_hash_directive(&mut self) -> Option<Statement<'arena>> {
        // Peek at the next token to determine which directive
        match &self.peek_token {
            Token::Include => self.parse_include_statement(),
            Token::Identifier(name) if name == "version" => {
                self.next_token(); // consume #
                self.next_token(); // consume "version", now at the build number
                match &self.cur_token {
                    Token::Integer(n) => {
                        let build = *n;
                        Some(Statement::Version { build, line: self.lexer.line })
                    }
                    _ => {
                        self.push_error(format!("Expected integer after #version, got {:?}", self.cur_token));
                        None
                    }
                }
            }
            _ => {
                self.push_error(format!("Unknown directive after #, got {:?}", self.peek_token));
                None
            }
        }
    }


    // --- Variable Declaration ---
    fn parse_let_statement(&mut self, mutable: bool) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        match &self.peek_token {
            Token::Identifier(name) => {
                let name = name.clone();
                self.next_token(); 

                // Optional type annotation: let x: int = ...
                let mut type_hint = None;
                if self.peek_token == Token::Colon {
                    self.next_token(); // consume :
                    self.next_token(); // consume type
                    if let Token::Identifier(t) = &self.cur_token {
                        type_hint = Some(t.clone());
                    }
                }

                if !self.expect_peek(Token::Equal) {
                    return None;
                }
                
                self.next_token();
                
                let value = self.parse_expression(Precedence::Lowest)?;
                
                if self.peek_token == Token::Semicolon {
                    self.next_token();
                }
                
                Some(Statement::Let { name, mutable, type_hint, value, line: start_line })
            }
            _ => {
                self.peek_error(Token::Identifier("name".to_string()));
                None
            }
        }
    }

    // --- Function Declaration (statement) ---
    fn parse_fn_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        // fn name(params) -> RetType { body }
        self.next_token(); // consume fn
        
        let name = match &self.cur_token {
            Token::Identifier(n) => n.clone(),
            Token::LParen => {
                // This is a lambda: fn() { ... } used as expression
                let params = self.parse_function_params()?;
                let mut return_type = "void".to_string();
                if self.peek_token == Token::Arrow {
                    self.next_token(); // ->
                    self.next_token(); // type
                    if let Token::Identifier(t) = &self.cur_token {
                        return_type = t.clone();
                    }
                }
                if !self.expect_peek(Token::LBrace) { return None; }
                let body = self.parse_block_statement()?;
                
                if self.peek_token == Token::Semicolon { self.next_token(); }
                
                return Some(Statement::Expression {
                    expression: Expression::FunctionLiteral {
                        parameters: params,
                        body: self.arena.alloc(body),
                        return_type,
                    },
                    line: start_line,
                });
            }
            _ => return None,
        };
        
        // Expect (
        if !self.expect_peek(Token::LParen) { return None; }
        
        let params = self.parse_function_params()?;
        
        // Return type
        let mut return_type = "void".to_string();
        if self.peek_token == Token::Arrow {
            self.next_token(); // ->
            self.next_token(); // type
            if let Token::Identifier(t) = &self.cur_token {
                return_type = t.clone();
            }
        }
        
        if !self.expect_peek(Token::LBrace) { return None; }
        
        let body = self.parse_block_statement()?;
        
        Some(Statement::Function {
            name,
            parameters: params,
            body: self.arena.alloc(body),
            return_type,
            line: start_line,
        })
    }

    fn parse_return_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        self.next_token(); 
        
        let value = if self.cur_token == Token::Semicolon || self.cur_token == Token::RBrace {
             None 
        } else {
             let expr = self.parse_expression(Precedence::Lowest)?;
             if self.peek_token == Token::Semicolon {
                 self.next_token();
             }
             Some(expr)
        };

        Some(Statement::Return { value, line: start_line })
    }
    
    // --- While ---
    fn parse_while_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        self.next_token(); // consume 'while'
        let condition = self.parse_expression(Precedence::Lowest)?;
        
        if !self.expect_peek(Token::LBrace) { return None; }
        let body = self.parse_block_statement()?;
        
        Some(Statement::While { condition, body: self.arena.alloc(body), line: start_line })
    }
    
    // --- For ---
    fn parse_for_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        self.next_token(); // consume 'for'
        
        let iterator = match &self.cur_token {
            Token::Identifier(name) => name.clone(),
            _ => return None,
        };
        self.next_token();
        
        // Expect 'in'
        if self.cur_token != Token::In {
            self.push_error(format!("Expected 'in' after for iterator, got {:?}", self.cur_token));
            return None;
        }
        self.next_token();
        
        let range = self.parse_expression(Precedence::Lowest)?;
        
        if !self.expect_peek(Token::LBrace) { return None; }
        let body = self.parse_block_statement()?;
        
        Some(Statement::For { iterator, range, body: self.arena.alloc(body), line: start_line })
    }
    
    // --- Include ---
    fn parse_include_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        // #include <system> as sys
        // #include "utils.nvr"
        self.next_token(); // consume #, now at Include
        
        if self.cur_token != Token::Include {
            return None;
        }
        self.next_token(); // now at path (String or Less)
        
        let path = match &self.cur_token {
            Token::String(s) => {
                s.clone()
            },
            Token::Less => {
                // Read until >
                let mut name = String::new();
                self.next_token();
                while self.cur_token != Token::Greater && self.cur_token != Token::EOF {
                    if let Token::Identifier(s) = &self.cur_token {
                        if !name.is_empty() { name.push('.'); }
                        name.push_str(s);
                    }
                    self.next_token();
                }
                // cur_token is now >, don't advance past it yet
                name
            }
            _ => return None,
        };
        
        // Check for 'as' alias: peek ahead
        let mut alias = None;
        if self.peek_token == Token::As {
            self.next_token(); // move to As
            self.next_token(); // move to alias identifier
            if let Token::Identifier(a) = &self.cur_token {
                alias = Some(a.clone());
            }
        }
        
        Some(Statement::Include { path, alias, line: start_line })
    }
    
    // --- Class ---
    fn parse_class_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        self.next_token(); // consume 'class'
        
        let name = match &self.cur_token {
            Token::Identifier(name) => name.clone(),
            _ => return None,
        };
        self.next_token();
        
        // Inheritance: class Player : Entity
        let mut parent = None;
        if self.cur_token == Token::Colon {
             self.next_token();
             if let Token::Identifier(p) = &self.cur_token {
                 parent = Some(p.clone());
                 self.next_token();
             }
        }
        
        if self.cur_token != Token::LBrace { return None; }
        
        let mut methods = vec![];
        let mut fields = vec![];
        
        self.next_token(); // skip {
        
        while self.cur_token != Token::RBrace && self.cur_token != Token::EOF {
            match &self.cur_token {
                Token::Fn => {
                    if let Some(stmt) = self.parse_fn_statement() {
                        methods.push(stmt);
                    }
                    // parse_fn_statement leaves cur_token at the method's closing }.
                    // Advance past it so the class loop continues correctly.
                    self.next_token();
                    continue;
                },
                Token::Pub => {
                    self.next_token();
                    if let Token::Identifier(n) = &self.cur_token {
                        let field_name = n.clone();
                        self.next_token();
                        if self.cur_token == Token::Colon {
                            self.next_token();
                            if let Token::Identifier(ty) = &self.cur_token {
                                fields.push((true, field_name, ty.clone()));
                                self.next_token();
                            }
                        }
                    }
                },
                Token::Mut => {
                    self.next_token();
                    if let Token::Identifier(n) = &self.cur_token {
                        let field_name = n.clone();
                        self.next_token();
                        if self.cur_token == Token::Colon {
                            self.next_token();
                            if let Token::Identifier(ty) = &self.cur_token {
                                fields.push((false, field_name, ty.clone()));
                                self.next_token();
                            }
                        }
                    }
                },
                Token::Identifier(_) => {
                    if let Token::Identifier(n) = &self.cur_token {
                        let field_name = n.clone();
                        self.next_token();
                        if self.cur_token == Token::Colon {
                            self.next_token();
                            if let Token::Identifier(ty) = &self.cur_token {
                                fields.push((false, field_name, ty.clone()));
                                self.next_token();
                            }
                        }
                    }
                },
                _ => { self.next_token(); }
            }
            if self.cur_token == Token::Semicolon { self.next_token(); }
        }
        
        Some(Statement::Class { name, parent, methods, fields, line: start_line })
    }
    
    // --- Struct ---
    fn parse_struct_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        self.next_token();
        let name = match &self.cur_token {
            Token::Identifier(name) => name.clone(),
            _ => return None,
        };
        self.next_token();
        
        if self.cur_token != Token::LBrace { return None; }
        self.next_token();
        
        let mut fields = vec![];
        while self.cur_token != Token::RBrace && self.cur_token != Token::EOF {
            if let Token::Identifier(n) = &self.cur_token {
                let name = n.clone();
                self.next_token();
                if self.cur_token == Token::Colon {
                    self.next_token();
                    if let Token::Identifier(ty) = &self.cur_token {
                        fields.push((name, ty.clone()));
                        self.next_token();
                    }
                }
                if self.cur_token == Token::Comma { self.next_token(); }
            } else {
                self.next_token();
            }
        }
        
        Some(Statement::Struct { name, fields, line: start_line })
    }

    fn parse_generics(&mut self) -> Vec<String> {
        let mut generics = vec![];
        if self.cur_token == Token::Less {
            self.next_token();
            while self.cur_token != Token::Greater && self.cur_token != Token::EOF {
                if let Token::Identifier(id) = &self.cur_token {
                    generics.push(id.clone());
                    self.next_token();
                }
                if self.cur_token == Token::Comma {
                    self.next_token();
                }
            }
            if self.cur_token == Token::Greater {
                self.next_token();
            }
        }
        generics
    }

    // --- Enum ---
    fn parse_enum_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        self.next_token(); // consume 'enum'
        let name = match &self.cur_token {
            Token::Identifier(n) => n.clone(),
            _ => return None,
        };
        self.next_token();
        let generics = self.parse_generics();
        
        if self.cur_token != Token::LBrace { return None; }
        self.next_token();
        
        let mut variants = vec![];
        while self.cur_token != Token::RBrace && self.cur_token != Token::EOF {
            if let Token::Identifier(vname) = &self.cur_token {
                let v = vname.clone();
                self.next_token();
                let mut payload = None;
                if self.cur_token == Token::LParen {
                    self.next_token();
                    if let Token::Identifier(ty) = &self.cur_token {
                        payload = Some(ty.clone());
                        self.next_token();
                    }
                    if self.cur_token == Token::RParen {
                        self.next_token();
                    }
                }
                variants.push((v, payload));
                if self.cur_token == Token::Comma {
                    self.next_token();
                }
            } else {
                self.next_token();
            }
        }
        if self.cur_token == Token::RBrace { self.next_token(); }
        
        Some(Statement::Enum { name, generics, variants, line: start_line })
    }

    // --- Trait ---
    fn parse_trait_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        self.next_token(); // consume 'trait'
        let name = match &self.cur_token {
            Token::Identifier(n) => n.clone(),
            _ => return None,
        };
        self.next_token();
        let generics = self.parse_generics();
        
        if self.cur_token != Token::LBrace { return None; }
        self.next_token();
        
        let mut methods = vec![];
        while self.cur_token != Token::RBrace && self.cur_token != Token::EOF {
            if self.cur_token == Token::Fn {
                self.next_token();
                if let Token::Identifier(mname) = &self.cur_token {
                    let m = mname.clone();
                    self.next_token();
                    
                    let mut params = vec![];
                    if self.cur_token == Token::LParen {
                        self.next_token();
                        while self.cur_token != Token::RParen && self.cur_token != Token::EOF {
                            if let Token::Identifier(pname) = &self.cur_token {
                                let pn = pname.clone();
                                self.next_token();
                                if self.cur_token == Token::Colon {
                                    self.next_token();
                                    if let Token::Identifier(pty) = &self.cur_token {
                                        params.push((pn, pty.clone()));
                                        self.next_token();
                                    }
                                }
                            }
                            if self.cur_token == Token::Comma { self.next_token(); }
                        }
                        if self.cur_token == Token::RParen { self.next_token(); }
                    }
                    
                    let mut ret_ty = "void".to_string();
                    if self.cur_token == Token::Arrow {
                        self.next_token();
                        if let Token::Identifier(rty) = &self.cur_token {
                            ret_ty = rty.clone();
                            self.next_token();
                        }
                    }
                    
                    if self.cur_token == Token::Semicolon { self.next_token(); }
                    
                    methods.push((m, params, ret_ty));
                }
            } else {
                self.next_token();
            }
        }
        if self.cur_token == Token::RBrace { self.next_token(); }
        
        Some(Statement::Trait { name, generics, methods, line: start_line })
    }

    // --- Impl ---
    fn parse_impl_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        self.next_token(); // consume 'impl'
        
        let generics = self.parse_generics();
        
        let first_name = match &self.cur_token {
            Token::Identifier(n) => n.clone(),
            _ => return None,
        };
        self.next_token();
        
        let mut trait_name = None;
        let mut target_name = first_name.clone();
        
        // Check for 'for' e.g. impl Trait for Struct
        if self.cur_token == Token::For {
            self.next_token();
            trait_name = Some(first_name);
            if let Token::Identifier(t) = &self.cur_token {
                target_name = t.clone();
                self.next_token();
            } else {
                return None;
            }
        }
        
        if self.cur_token != Token::LBrace { return None; }
        self.next_token();
        
        let mut methods = vec![];
        while self.cur_token != Token::RBrace && self.cur_token != Token::EOF {
            if self.cur_token == Token::Fn {
                if let Some(stmt) = self.parse_fn_statement() {
                    methods.push(stmt);
                }
            } else {
                self.next_token();
            }
        }
        if self.cur_token == Token::RBrace { self.next_token(); }
        
        Some(Statement::Impl { trait_name, target_name, generics, methods, line: start_line })
    }

    fn parse_expression_statement(&mut self) -> Option<Statement<'arena>> {
        let start_line = self.lexer.line;
        let expr = self.parse_expression(Precedence::Lowest)?;
        
        if self.peek_token == Token::Semicolon {
            self.next_token();
        }
        
        Some(Statement::Expression { expression: expr, line: start_line })
    }

    // --- Expression Parsing ---
    fn parse_expression(&mut self, precedence: Precedence) -> Option<Expression<'arena>> {
        let mut left = self.parse_prefix()?;

        // Assignment: if next token is = and we parsed an identifier or member access
        if self.peek_token == Token::Equal {
            match &left {
                Expression::Identifier(_) | Expression::MemberAccess { .. } | Expression::Index { .. } => {
                    self.next_token(); // consume =
                    self.next_token(); // move to value
                    let value = self.parse_expression(Precedence::Lowest)?;
                    return Some(Expression::Assign {
                        target: self.arena.alloc(left),
                        value: self.arena.alloc(value),
                    });
                },
                _ => {}
            }
        }

        while self.peek_token != Token::Semicolon 
              && self.peek_token != Token::LBrace
              && precedence < self.peek_precedence() 
        {
            match self.peek_token {
                Token::Plus | Token::Minus | Token::Star | Token::Slash | Token::Percent |
                Token::EqualEqual | Token::NotEqual | Token::Less | Token::Greater |
                Token::LessEqual | Token::GreaterEqual |
                Token::And | Token::Or |
                Token::LParen | Token::LBracket | Token::Dot | Token::DotDot => {
                    self.next_token();
                    left = self.parse_infix(left)?;
                },
                _ => return Some(left),
            }
        }
        Some(left)
    }

    fn parse_prefix(&mut self) -> Option<Expression<'arena>> {
        match &self.cur_token {
            Token::Identifier(name) => Some(Expression::Identifier(name.clone())),
            Token::Integer(val) => Some(Expression::Integer(*val)),
            Token::Float(val) => Some(Expression::Float(*val)),
            Token::String(val) => Some(Expression::String(val.clone())),
            Token::True => Some(Expression::Boolean(true)),
            Token::False => Some(Expression::Boolean(false)),
            Token::Null => Some(Expression::Null),
            Token::Minus | Token::Bang => {
                let op = if self.cur_token == Token::Minus { "-" } else { "!" }.to_string();
                self.next_token();
                let right = self.parse_expression(Precedence::Prefix)?;
                Some(Expression::Prefix { operator: op, right: self.arena.alloc(right) })
            },
            Token::Ampersand => {
                self.next_token();
                let mut op = "&".to_string();
                if self.cur_token == Token::Mut {
                    op = "&mut".to_string();
                    self.next_token();
                }
                let right = self.parse_expression(Precedence::Prefix)?;
                Some(Expression::Prefix { operator: op, right: self.arena.alloc(right) })
            },
            Token::LParen => {
                self.next_token();
                let expr = self.parse_expression(Precedence::Lowest)?;
                if !self.expect_peek(Token::RParen) { return None; }
                Some(expr)
            },
            Token::If => self.parse_if_expression(),
            Token::Match => self.parse_match_expression(),
            Token::Fn => self.parse_function_literal(), 
            Token::LBracket => self.parse_array_literal(),
            _ => {
                self.push_error(format!("No prefix parse function for {:?}", self.cur_token));
                None
            }
        }
    }

    fn parse_infix(&mut self, left: Expression<'arena>) -> Option<Expression<'arena>> {
        match &self.cur_token {
            Token::LParen => return self.parse_call_expression(left),
            Token::LBracket => return self.parse_index_expression(left),
            Token::QuestionMark => {
                self.next_token();
                let node = Expression::Try { value: self.arena.alloc(left) };
                return Some(node);
            },
            Token::Dot => {
                self.next_token();
                if let Token::Identifier(member) = &self.cur_token {
                    return Some(Expression::MemberAccess {
                        object: self.arena.alloc(left),
                        member: member.clone(),
                    });
                }
                return None;
            },
            Token::DotDot => {
                let precedence = self.cur_precedence();
                self.next_token();
                let end = self.parse_expression(precedence)?;
                return Some(Expression::Range {
                    start: self.arena.alloc(left),
                    end: self.arena.alloc(end),
                });
            },
            _ => {}
        }

        let operator = match self.cur_token {
            Token::Plus => "+",
            Token::Minus => "-",
            Token::Star => "*",
            Token::Slash => "/",
            Token::Percent => "%",
            Token::EqualEqual => "==",
            Token::NotEqual => "!=",
            Token::Less => "<",
            Token::Greater => ">",
            Token::LessEqual => "<=",
            Token::GreaterEqual => ">=",
            Token::And => "&&",
            Token::Or => "||",
            _ => return None,
        }.to_string();

        let precedence = self.cur_precedence();
        self.next_token();
        let right = self.parse_expression(precedence)?;

        Some(Expression::Infix {
            left: self.arena.alloc(left),
            operator,
            right: self.arena.alloc(right),
        })
    }
    
    fn parse_call_expression(&mut self, function: Expression<'arena>) -> Option<Expression<'arena>> {
        let arguments = self.parse_expression_list(Token::RParen)?;
        Some(Expression::Call { function: self.arena.alloc(function), arguments })
    }
    
    fn parse_index_expression(&mut self, left: Expression<'arena>) -> Option<Expression<'arena>> {
        self.next_token();
        let index = self.parse_expression(Precedence::Lowest)?;
        if !self.expect_peek(Token::RBracket) { return None; }
        Some(Expression::Index { left: self.arena.alloc(left), index: self.arena.alloc(index) })
    }
    
    fn parse_array_literal(&mut self) -> Option<Expression<'arena>> {
        let elements = self.parse_expression_list(Token::RBracket)?;
        Some(Expression::ArrayLiteral(elements))
    }

    fn parse_expression_list(&mut self, end_token: Token) -> Option<Vec<Expression<'arena>>> {
        let mut list = vec![];

        if self.peek_token == end_token {
            self.next_token();
            return Some(list);
        }

        self.next_token();
        list.push(self.parse_expression(Precedence::Lowest)?);

        while self.peek_token == Token::Comma {
            self.next_token();
            self.next_token();
            list.push(self.parse_expression(Precedence::Lowest)?);
        }

        if !self.expect_peek(end_token) { return None; }
        Some(list)
    }

    fn parse_if_expression(&mut self) -> Option<Expression<'arena>> {
        self.next_token(); // skip if
        let condition = self.parse_expression(Precedence::Lowest)?;
        
        if !self.expect_peek(Token::LBrace) { return None; }
        let consequence = self.parse_block_statement()?;
        
        let mut alternative = None;
        if self.peek_token == Token::Else {
            self.next_token();
            
            // else if { ... } or else { ... }
            if self.peek_token == Token::If {
                self.next_token(); // move to if
                let else_if = self.parse_if_expression()?;
                let alt = Statement::Block { 
                    statements: vec![Statement::Expression { expression: else_if, line: self.lexer.line }],
                    line: self.lexer.line,
                };
                alternative = Some(alt);
            } else if self.expect_peek(Token::LBrace) {
                alternative = Some(self.parse_block_statement()?);
            }
        }
        
        Some(Expression::If {
            condition: self.arena.alloc(condition),
            consequence: self.arena.alloc(consequence),
            alternative: alternative.map(|a| &*self.arena.alloc(a)),
        })
    }

    fn parse_match_expression(&mut self) -> Option<Expression<'arena>> {
        self.next_token(); // Skip 'match'
        let value = self.parse_expression(Precedence::Lowest)?;
        
        if !self.expect_peek(Token::LBrace) { return None; }
        self.next_token(); // now inside block
        
        let mut arms = vec![];
        while self.cur_token != Token::RBrace && self.cur_token != Token::EOF {
            let pattern = self.parse_expression(Precedence::Lowest)?;
            if !self.expect_peek(Token::FatArrow) { return None; }
            self.next_token(); // advance to start of expression/statement
            
            let stmt = self.parse_statement()?;
            arms.push((pattern, self.arena.alloc(stmt) as &'arena Statement<'arena>));
            
            self.next_token(); // advance past last token of stmt 
            
            // if we are at a comma, consume it
            if self.cur_token == Token::Comma {
                self.next_token();
            }
        }
        
        Some(Expression::Match {
            value: self.arena.alloc(value),
            arms,
        })
    }
    
    fn parse_block_statement(&mut self) -> Option<Statement<'arena>> {
        let mut statements = vec![];
        self.next_token();
        
        while self.cur_token != Token::RBrace && self.cur_token != Token::EOF {
            if let Some(stmt) = self.parse_statement() {
                statements.push(stmt);
            }
            self.next_token();
        }
        
        Some(Statement::Block { statements, line: self.lexer.line })
    }
    
    fn parse_function_literal(&mut self) -> Option<Expression<'arena>> {
        if !self.expect_peek(Token::LParen) { return None; }
        let params = self.parse_function_params()?;

        let mut return_type = "void".to_string();
        if self.peek_token == Token::Arrow {
            self.next_token(); // ->
            self.next_token(); // type
            if let Token::Identifier(t) = &self.cur_token {
                return_type = t.clone();
            }
        }
        
        if !self.expect_peek(Token::LBrace) { return None; }
        let body = self.parse_block_statement()?;
        
        Some(Expression::FunctionLiteral { 
            parameters: params, body: self.arena.alloc(body), return_type 
        })
    }

    fn parse_function_params(&mut self) -> Option<Vec<(String, String)>> {
        let mut params = vec![];
        
        if self.peek_token == Token::RParen {
            self.next_token();
            return Some(params);
        }
        
        self.next_token();
        
        let parse_one = |p: &mut Parser<'src, 'arena>| -> Option<(String, String)> {
            if let Token::Identifier(name) = &p.cur_token {
                let name = name.clone();
                let mut ty = "Any".to_string();
                if p.peek_token == Token::Colon {
                    p.next_token();
                    p.next_token();
                    if let Token::Identifier(t) = &p.cur_token {
                        ty = t.clone();
                    }
                }
                Some((name, ty))
            } else {
                None
            }
        };
        
        if let Some(param) = parse_one(self) {
            params.push(param);
        }
        
        while self.peek_token == Token::Comma {
            self.next_token();
            self.next_token();
            if let Some(param) = parse_one(self) {
                params.push(param);
            }
        }
        
        if !self.expect_peek(Token::RParen) { return None; }
        Some(params)
    }

    fn peek_precedence(&self) -> Precedence { self.token_precedence(&self.peek_token) }
    fn cur_precedence(&self) -> Precedence { self.token_precedence(&self.cur_token) }
    
    fn token_precedence(&self, token: &Token) -> Precedence {
        match token {
            Token::EqualEqual | Token::NotEqual => Precedence::Equals,
            Token::Less | Token::Greater | Token::LessEqual | Token::GreaterEqual => Precedence::LessGreater,
            Token::And | Token::Or => Precedence::Equals, // logical
            Token::Plus | Token::Minus => Precedence::Sum,
            Token::Star | Token::Slash | Token::Percent => Precedence::Product,
            Token::LParen => Precedence::Call,
            Token::LBracket => Precedence::Index,
            Token::Dot => Precedence::Member,
            Token::DotDot => Precedence::Sum, // Range has Sum-level precedence
            Token::QuestionMark => Precedence::Member,
            _ => Precedence::Lowest,
        }
    }

    fn expect_peek(&mut self, token: Token) -> bool {
        if self.peek_token == token {
            self.next_token();
            true
        } else {
            self.peek_error(token);
            false
        }
    }
    
    pub fn push_error(&mut self, msg: String) {
        self.errors.push(format!("Line {}: {}", self.lexer.line, msg));
    }

    fn peek_error(&mut self, token: Token) {
        self.push_error(format!("Expected next token to be {:?}, got {:?} instead", token, self.peek_token));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::ast::*;
    
    fn parse(input: &str) -> (Bump, Vec<String>) {
        let arena = Bump::new();
        // Safety: we transmute the lifetime so the program can outlive this function.
        // In tests only — the arena is returned alongside the data.
        let arena_ref: &Bump = unsafe { &*(&arena as *const Bump) };
        let l = Lexer::new(input);
        let mut p = Parser::new(l, arena_ref);
        let prog = p.parse_program();
        let errors = p.errors.clone();
        if !errors.is_empty() {
            panic!("Parser errors: {:?}", errors);
        }
        // We just validate statement count via the parsed program.
        // Since the arena owns the data and we return it, this is safe for testing.
        (arena, errors)
    }

    fn parse_and_check(input: &str) -> usize {
        let arena = Bump::new();
        let l = Lexer::new(input);
        let mut p = Parser::new(l, &arena);
        let prog = p.parse_program();
        assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
        prog.statements.len()
    }
    
    #[test]
    fn test_let_statement() {
        let arena = Bump::new();
        let l = Lexer::new("let x = 5;");
        let mut p = Parser::new(l, &arena);
        let prog = p.parse_program();
        assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Let { name, mutable, .. } => {
                assert_eq!(name, "x");
                assert!(!mutable);
            },
            _ => panic!("Expected Let statement"),
        }
    }
    
    #[test]
    fn test_mut_statement() {
        let arena = Bump::new();
        let l = Lexer::new("mut counter = 0;");
        let mut p = Parser::new(l, &arena);
        let prog = p.parse_program();
        assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Let { name, mutable, .. } => {
                assert_eq!(name, "counter");
                assert!(mutable);
            },
            _ => panic!("Expected Let(mut) statement"),
        }
    }
    
    #[test]
    fn test_fn_declaration() {
        let arena = Bump::new();
        let l = Lexer::new("fn add(a: int, b: int) -> int { return a + b; }");
        let mut p = Parser::new(l, &arena);
        let prog = p.parse_program();
        assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Function { name, parameters, return_type, .. } => {
                assert_eq!(name, "add");
                assert_eq!(parameters.len(), 2);
                assert_eq!(parameters[0], ("a".to_string(), "int".to_string()));
                assert_eq!(parameters[1], ("b".to_string(), "int".to_string()));
                assert_eq!(return_type, "int");
            },
            _ => panic!("Expected Function statement"),
        }
    }
    
    #[test]
    fn test_while_loop() {
        assert_eq!(parse_and_check("while x > 0 { x = x - 1; }"), 1);
    }
    
    #[test]
    fn test_for_loop() {
        let arena = Bump::new();
        let l = Lexer::new("for i in items { print(i); }");
        let mut p = Parser::new(l, &arena);
        let prog = p.parse_program();
        assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::For { iterator, .. } => assert_eq!(iterator, "i"),
            _ => panic!("Expected For statement"),
        }
    }
    
    #[test]
    fn test_struct_definition() {
        let arena = Bump::new();
        let l = Lexer::new("struct Vector2 { x: float, y: float }");
        let mut p = Parser::new(l, &arena);
        let prog = p.parse_program();
        assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Struct { name, fields, .. } => {
                assert_eq!(name, "Vector2");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0], ("x".to_string(), "float".to_string()));
                assert_eq!(fields[1], ("y".to_string(), "float".to_string()));
            },
            _ => panic!("Expected Struct"),
        }
    }
    
    #[test]
    fn test_class_with_method() {
        let arena = Bump::new();
        let l = Lexer::new("class Player { fn jump() { } }");
        let mut p = Parser::new(l, &arena);
        let prog = p.parse_program();
        assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Class { name, methods, .. } => {
                assert_eq!(name, "Player");
                assert_eq!(methods.len(), 1);
            },
            _ => panic!("Expected Class"),
        }
    }
    
    #[test]
    fn test_member_access() {
        let arena = Bump::new();
        let l = Lexer::new("player.hp;");
        let mut p = Parser::new(l, &arena);
        let prog = p.parse_program();
        assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Expression { expression, .. } => {
                assert!(matches!(expression, Expression::MemberAccess { .. }));
            },
            _ => panic!("Expected expression"),
        }
    }

    #[test]
    fn test_array_literal() {
        let arena = Bump::new();
        let l = Lexer::new("let arr = [1, 2, 3];");
        let mut p = Parser::new(l, &arena);
        let prog = p.parse_program();
        assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Let { value, .. } => {
                match value {
                    Expression::ArrayLiteral(elems) => assert_eq!(elems.len(), 3),
                    _ => panic!("Expected array literal"),
                }         
            },
            _ => panic!("Expected let"),
        }
    }
    
    #[test]
    fn test_comments_handled() {
        let arena = Bump::new();
        let l = Lexer::new("// this is a comment\nlet x = 42;");
        let mut p = Parser::new(l, &arena);
        let prog = p.parse_program();
        assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Let { name, .. } => assert_eq!(name, "x"),
            _ => panic!("Expected let"),
        }
    }

    /// Parser speed benchmark.
    /// Run with: cargo test -p kinetix-language bench_parser_speed -- --nocapture
    #[test]
    fn bench_parser_speed() {
        // Generate a large synthetic Kinetix source file
        let mut source = String::with_capacity(500_000);

        // Add 500 variable declarations
        for i in 0..500 {
            source.push_str(&format!("let var_{} = {};\n", i, i * 7 + 3));
        }

        // Add 200 function declarations
        for i in 0..200 {
            source.push_str(&format!(
                "fn func_{}(a: int, b: int) -> int {{\n  let result = a + b * {};\n  return result;\n}}\n",
                i, i
            ));
        }

        // Add 50 class declarations
        for i in 0..50 {
            source.push_str(&format!(
                "class Entity_{} {{\n  pub x: float\n  pub y: float\n  fn update() {{ let dx = x + {}; }}\n}}\n",
                i, i
            ));
        }

        // Add 200 for loops
        for i in 0..200 {
            source.push_str(&format!(
                "for i in 0..{} {{\n  let tmp = i * {};\n}}\n",
                i + 10, i + 1
            ));
        }

        // Add 200 while loops
        for i in 0..200 {
            source.push_str(&format!(
                "mut w_{} = {};\nwhile w_{} > 0 {{\n  w_{} = w_{} - 1;\n}}\n",
                i, i * 5, i, i, i
            ));
        }

        // Add 500 expression statements (function calls, math)
        for i in 0..500 {
            source.push_str(&format!("println({} + {} * {});\n", i, i + 1, i + 2));
        }

        // Add 100 if/else blocks
        for i in 0..100 {
            source.push_str(&format!(
                "if {} > {} {{ let yes = true; }} else {{ let no = false; }}\n",
                i * 2, i
            ));
        }

        // Add 100 array literals
        for i in 0..100 {
            source.push_str(&format!(
                "let arr_{} = [{}, {}, {}, {}, {}];\n",
                i, i, i + 1, i + 2, i + 3, i + 4
            ));
        }

        let line_count = source.lines().count();
        let byte_count = source.len();

        println!("\n══════════════════════════════════════════");
        println!("  Parser Speed Benchmark");
        println!("══════════════════════════════════════════");
        println!("  Source: {} lines, {} bytes ({:.1} KB)",
            line_count, byte_count, byte_count as f64 / 1024.0);

        // Warm up
        for _ in 0..3 {
            let arena = Bump::new();
            let l = Lexer::new(&source);
            let mut p = Parser::new(l, &arena);
            let _ = p.parse_program();
        }

        // Benchmark: 10 iterations
        let iterations = 10;
        let start = std::time::Instant::now();
        let mut stmts = 0;

        for _ in 0..iterations {
            let arena = Bump::new();
            let l = Lexer::new(&source);
            let mut p = Parser::new(l, &arena);
            let prog = p.parse_program();
            assert!(p.errors.is_empty(), "Parser errors: {:?}", p.errors);
            stmts = prog.statements.len();
        }

        let elapsed = start.elapsed();
        let avg = elapsed / iterations;
        let lines_per_sec = (line_count as f64 / avg.as_secs_f64()) as u64;
        let bytes_per_sec = (byte_count as f64 / avg.as_secs_f64()) as u64;

        println!("  Statements: {}", stmts);
        println!("  Iterations: {}", iterations);
        println!("──────────────────────────────────────────");
        println!("  Total time:  {:.2?}", elapsed);
        println!("  Average:     {:.2?} per parse", avg);
        println!("  Throughput:  {} lines/sec", lines_per_sec);
        println!("  Throughput:  {:.1} MB/sec", bytes_per_sec as f64 / 1_048_576.0);
        println!("══════════════════════════════════════════\n");
    }
}
