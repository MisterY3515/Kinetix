
use crate::lexer::{Lexer, Token};
use crate::ast::{Program, Statement, Expression};

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

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    cur_token: Token,
    peek_token: Token,
    pub errors: Vec<String>,
}

impl<'a> Parser<'a> {
    pub fn new(lexer: Lexer<'a>) -> Self {
        let mut p = Parser {
            lexer,
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

    pub fn parse_program(&mut self) -> Program {
        let mut program = Program::new();

        while self.cur_token != Token::EOF {
            if let Some(stmt) = self.parse_statement() {
                program.statements.push(stmt);
            }
            self.next_token();
        }
        program
    }

    fn parse_statement(&mut self) -> Option<Statement> {
        match self.cur_token {
            Token::Let => self.parse_let_statement(false),
            Token::Mut => self.parse_let_statement(true),
            Token::Fn => self.parse_fn_statement(),
            Token::Return => self.parse_return_statement(),
            Token::While => self.parse_while_statement(),
            Token::For => self.parse_for_statement(),
            Token::Class => self.parse_class_statement(),
            Token::Struct => self.parse_struct_statement(),
            Token::Hash => self.parse_hash_directive(),
            Token::Break => Some(Statement::Break),
            Token::Continue => Some(Statement::Continue),
            _ => self.parse_expression_statement(),
        }
    }

    // --- Hash Directives (#include, #version) ---
    fn parse_hash_directive(&mut self) -> Option<Statement> {
        // Peek at the next token to determine which directive
        match &self.peek_token {
            Token::Include => self.parse_include_statement(),
            Token::Identifier(name) if name == "version" => {
                self.next_token(); // consume #
                self.next_token(); // consume "version", now at the build number
                match &self.cur_token {
                    Token::Integer(n) => {
                        let build = *n;
                        Some(Statement::Version { build })
                    }
                    _ => {
                        self.errors.push(format!("Expected integer after #version, got {:?}", self.cur_token));
                        None
                    }
                }
            }
            _ => {
                self.errors.push(format!("Unknown directive after #, got {:?}", self.peek_token));
                None
            }
        }
    }


    // --- Variable Declaration ---
    fn parse_let_statement(&mut self, mutable: bool) -> Option<Statement> {
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
                
                Some(Statement::Let { name, mutable, type_hint, value })
            }
            _ => {
                self.peek_error(Token::Identifier("name".to_string()));
                None
            }
        }
    }

    // --- Function Declaration (statement) ---
    fn parse_fn_statement(&mut self) -> Option<Statement> {
        // fn name(params) -> RetType { body }
        self.next_token(); // consume fn
        
        let name = match &self.cur_token {
            Token::Identifier(n) => n.clone(),
            Token::LParen => {
                // This is a lambda: fn() { ... } used as expression
                // Backtrack: reparse as expression statement
                // Actually we can't easily backtrack. Let's detect:
                // If current token after fn is '(' -> it's a lambda expression.
                // We parse it inline.
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
                        body: Box::new(body),
                        return_type,
                    }
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
            body: Box::new(body),
            return_type,
        })
    }

    fn parse_return_statement(&mut self) -> Option<Statement> {
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

        Some(Statement::Return { value })
    }
    
    // --- While ---
    fn parse_while_statement(&mut self) -> Option<Statement> {
        self.next_token(); // consume 'while'
        let condition = self.parse_expression(Precedence::Lowest)?;
        
        if !self.expect_peek(Token::LBrace) { return None; }
        let body = self.parse_block_statement()?;
        
        Some(Statement::While { condition, body: Box::new(body) })
    }
    
    // --- For ---
    fn parse_for_statement(&mut self) -> Option<Statement> {
        self.next_token(); // consume 'for'
        
        let iterator = match &self.cur_token {
            Token::Identifier(name) => name.clone(),
            _ => return None,
        };
        self.next_token();
        
        // Expect 'in'
        if self.cur_token != Token::In {
            self.errors.push(format!("Expected 'in' after for iterator, got {:?}", self.cur_token));
            return None;
        }
        self.next_token();
        
        let range = self.parse_expression(Precedence::Lowest)?;
        
        if !self.expect_peek(Token::LBrace) { return None; }
        let body = self.parse_block_statement()?;
        
        Some(Statement::For { iterator, range, body: Box::new(body) })
    }
    
    // --- Include ---
    fn parse_include_statement(&mut self) -> Option<Statement> {
        // #include <system> as sys
        // #include "utils.nvr"
        self.next_token(); // consume #, now at Include
        
        if self.cur_token != Token::Include {
            return None;
        }
        self.next_token(); // now at path (String or Less)
        
        let path = match &self.cur_token {
            Token::String(s) => {
                // cur_token is now STRING. Don't advance further, 
                // parse_program's next_token() will do that.
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
        
        Some(Statement::Include { path, alias })
    }
    
    // --- Class ---
    fn parse_class_statement(&mut self) -> Option<Statement> {
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
        
        Some(Statement::Class { name, parent, methods, fields })
    }
    
    // --- Struct ---
    fn parse_struct_statement(&mut self) -> Option<Statement> {
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
        
        Some(Statement::Struct { name, fields })
    }

    fn parse_expression_statement(&mut self) -> Option<Statement> {
        let expr = self.parse_expression(Precedence::Lowest)?;
        
        if self.peek_token == Token::Semicolon {
            self.next_token();
        }
        
        Some(Statement::Expression { expression: expr })
    }

    // --- Expression Parsing ---
    fn parse_expression(&mut self, precedence: Precedence) -> Option<Expression> {
        let mut left = self.parse_prefix()?;

        // Assignment: if next token is = and we parsed an identifier or member access
        if self.peek_token == Token::Equal {
            match &left {
                Expression::Identifier(_) | Expression::MemberAccess { .. } | Expression::Index { .. } => {
                    self.next_token(); // consume =
                    self.next_token(); // move to value
                    let value = self.parse_expression(Precedence::Lowest)?;
                    return Some(Expression::Assign {
                        target: Box::new(left),
                        value: Box::new(value),
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

    fn parse_prefix(&mut self) -> Option<Expression> {
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
                Some(Expression::Prefix { operator: op, right: Box::new(right) })
            },
            Token::LParen => {
                self.next_token();
                let expr = self.parse_expression(Precedence::Lowest)?;
                if !self.expect_peek(Token::RParen) { return None; }
                Some(expr)
            },
            Token::If => self.parse_if_expression(),
            Token::Fn => self.parse_function_literal(), 
            Token::LBracket => self.parse_array_literal(),
            _ => {
                self.errors.push(format!("No prefix parse function for {:?}", self.cur_token));
                None
            }
        }
    }

    fn parse_infix(&mut self, left: Expression) -> Option<Expression> {
        match &self.cur_token {
            Token::LParen => return self.parse_call_expression(left),
            Token::LBracket => return self.parse_index_expression(left),
            Token::Dot => {
                self.next_token();
                if let Token::Identifier(member) = &self.cur_token {
                    return Some(Expression::MemberAccess {
                        object: Box::new(left),
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
                    start: Box::new(left),
                    end: Box::new(end),
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
            left: Box::new(left),
            operator,
            right: Box::new(right),
        })
    }
    
    fn parse_call_expression(&mut self, function: Expression) -> Option<Expression> {
        let arguments = self.parse_expression_list(Token::RParen)?;
        Some(Expression::Call { function: Box::new(function), arguments })
    }
    
    fn parse_index_expression(&mut self, left: Expression) -> Option<Expression> {
        self.next_token();
        let index = self.parse_expression(Precedence::Lowest)?;
        if !self.expect_peek(Token::RBracket) { return None; }
        Some(Expression::Index { left: Box::new(left), index: Box::new(index) })
    }
    
    fn parse_array_literal(&mut self) -> Option<Expression> {
        let elements = self.parse_expression_list(Token::RBracket)?;
        Some(Expression::ArrayLiteral(elements))
    }

    fn parse_expression_list(&mut self, end_token: Token) -> Option<Vec<Expression>> {
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

    fn parse_if_expression(&mut self) -> Option<Expression> {
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
                alternative = Some(Statement::Block { 
                    statements: vec![Statement::Expression { expression: else_if }]
                });
            } else if self.expect_peek(Token::LBrace) {
                alternative = Some(self.parse_block_statement()?);
            }
        }
        
        Some(Expression::If {
            condition: Box::new(condition),
            consequence: Box::new(consequence),
            alternative: alternative.map(Box::new),
        })
    }
    
    fn parse_block_statement(&mut self) -> Option<Statement> {
        let mut statements = vec![];
        self.next_token();
        
        while self.cur_token != Token::RBrace && self.cur_token != Token::EOF {
            if let Some(stmt) = self.parse_statement() {
                statements.push(stmt);
            }
            self.next_token();
        }
        
        Some(Statement::Block { statements })
    }
    
    fn parse_function_literal(&mut self) -> Option<Expression> {
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
            parameters: params, body: Box::new(body), return_type 
        })
    }

    fn parse_function_params(&mut self) -> Option<Vec<(String, String)>> {
        let mut params = vec![];
        
        if self.peek_token == Token::RParen {
            self.next_token();
            return Some(params);
        }
        
        self.next_token();
        
        let parse_one = |p: &mut Parser| -> Option<(String, String)> {
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
    
    fn peek_error(&mut self, token: Token) {
        self.errors.push(format!("Expected next token to be {:?}, got {:?} instead", token, self.peek_token));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::ast::*;
    
    fn parse(input: &str) -> Program {
        let l = Lexer::new(input);
        let mut p = Parser::new(l);
        let prog = p.parse_program();
        if !p.errors.is_empty() {
            panic!("Parser errors: {:?}", p.errors);
        }
        prog
    }
    
    #[test]
    fn test_let_statement() {
        let prog = parse("let x = 5;");
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Let { name, mutable, value, .. } => {
                assert_eq!(name, "x");
                assert!(!mutable);
                assert_eq!(value, &Expression::Integer(5));
            },
            _ => panic!("Expected Let statement"),
        }
    }
    
    #[test]
    fn test_mut_statement() {
        let prog = parse("mut counter = 0;");
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
        let prog = parse("fn add(a: int, b: int) -> int { return a + b; }");
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
        let prog = parse("while x > 0 { x = x - 1; }");
        assert_eq!(prog.statements.len(), 1);
        assert!(matches!(&prog.statements[0], Statement::While { .. }));
    }
    
    #[test]
    fn test_for_loop() {
        let prog = parse("for i in items { print(i); }");
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::For { iterator, .. } => assert_eq!(iterator, "i"),
            _ => panic!("Expected For statement"),
        }
    }
    
    #[test]
    fn test_struct_definition() {
        let prog = parse("struct Vector2 { x: float, y: float }");
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Struct { name, fields } => {
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
        let prog = parse("class Player { fn jump() { } }");
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
        let prog = parse("player.hp;");
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Expression { expression } => {
                assert!(matches!(expression, Expression::MemberAccess { .. }));
            },
            _ => panic!("Expected expression"),
        }
    }

    #[test]
    fn test_array_literal() {
        let prog = parse("let arr = [1, 2, 3];");
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
        let prog = parse("// this is a comment\nlet x = 42;");
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0] {
            Statement::Let { name, .. } => assert_eq!(name, "x"),
            _ => panic!("Expected let"),
        }
    }
}
