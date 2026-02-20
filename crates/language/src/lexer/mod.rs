
#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    // Keywords
    Let,
    Mut,
    Fn,
    Return,
    If,
    Else,
    While,
    For,
    In,
    Class,
    Struct,
    Import,
    Include,
    Pub,
    True,
    False,
    Null,
    Break,
    Continue,
    As,
    Match,
    
    // Literals
    Identifier(String),
    Integer(i64),
    Float(f64),
    String(String),
    
    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equal,
    EqualEqual,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Bang,
    And,      // &&
    Or,       // ||
    Dot,
    DotDot,   // ..  (Range)
    
    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    Arrow,    // ->
    FatArrow, // =>
    Hash,     // #
    
    EOF,
    Illegal,
}

pub struct Lexer<'a> {
    input: &'a str,
    position: usize,
    read_position: usize,
    ch: Option<char>,
    pub line: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut l = Lexer {
            input,
            position: 0,
            read_position: 0,
            ch: None,
            line: 1,
        };
        l.read_char();
        l
    }

    fn read_char(&mut self) {
        if self.ch == Some('\n') {
            self.line += 1;
        }
        if self.read_position >= self.input.len() {
            self.ch = None;
        } else {
            self.ch = self.input[self.read_position..].chars().next();
        }
        self.position = self.read_position;
        if let Some(ch) = self.ch {
            self.read_position += ch.len_utf8();
        } else {
            self.read_position += 1;
        }
    }

    fn peek_char(&self) -> Option<char> {
        if self.read_position >= self.input.len() {
            None
        } else {
            self.input[self.read_position..].chars().next()
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.ch {
            if ch.is_whitespace() {
                self.read_char();
            } else {
                break;
            }
        }
    }
    
    fn skip_comment(&mut self) {
        // Single-line comment: //
        // Already consumed first /
        self.read_char(); // consume second /
        while let Some(ch) = self.ch {
            if ch == '\n' {
                break;
            }
            self.read_char();
        }
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        
        let token = match self.ch {
            Some('#') => Token::Hash,
            Some('=') => {
                if self.peek_char() == Some('=') {
                    self.read_char();
                    Token::EqualEqual
                } else if self.peek_char() == Some('>') {
                    self.read_char();
                    Token::FatArrow
                } else {
                    Token::Equal
                }
            },
            Some('+') => Token::Plus,
            Some('-') => {
                if self.peek_char() == Some('>') {
                    self.read_char();
                    Token::Arrow
                } else {
                    Token::Minus
                }
            },
            Some('*') => Token::Star,
            Some('/') => {
                if self.peek_char() == Some('/') {
                    self.skip_comment();
                    return self.next_token(); // Recurse after comment
                } else {
                    Token::Slash
                }
            },
            Some('%') => Token::Percent,
            Some('.') => {
                if self.peek_char() == Some('.') {
                    self.read_char();
                    Token::DotDot
                } else {
                    Token::Dot
                }
            },
            Some('(') => Token::LParen,
            Some(')') => Token::RParen,
            Some('{') => Token::LBrace,
            Some('}') => Token::RBrace,
            Some('[') => Token::LBracket,
            Some(']') => Token::RBracket,
            Some(',') => Token::Comma,
            Some(';') => Token::Semicolon,
            Some(':') => Token::Colon,
            Some('<') => {
                 if self.peek_char() == Some('=') {
                    self.read_char();
                    Token::LessEqual
                } else {
                    Token::Less
                }
            },
            Some('>') => {
                 if self.peek_char() == Some('=') {
                    self.read_char();
                    Token::GreaterEqual
                } else {
                    Token::Greater
                }
            },
            Some('!') => {
                 if self.peek_char() == Some('=') {
                    self.read_char();
                    Token::NotEqual
                } else {
                    Token::Bang
                }
            },
            Some('&') => {
                if self.peek_char() == Some('&') {
                    self.read_char();
                    Token::And
                } else {
                    Token::Illegal
                }
            },
            Some('|') => {
                if self.peek_char() == Some('|') {
                    self.read_char();
                    Token::Or
                } else {
                    Token::Illegal
                }
            },
            Some('"') => return self.read_string(),
            Some(ch) => {
                if is_letter(ch) {
                    let ident = self.read_identifier();
                    return match ident.as_str() {
                        "let" => Token::Let,
                        "mut" => Token::Mut,
                        "fn" => Token::Fn,
                        "return" => Token::Return,
                        "if" => Token::If,
                        "else" => Token::Else,
                        "while" => Token::While,
                        "for" => Token::For,
                        "in" => Token::In,
                        "class" => Token::Class,
                        "struct" => Token::Struct,
                        "import" => Token::Import,
                        "include" => Token::Include,
                        "pub" => Token::Pub,
                        "true" => Token::True,
                        "false" => Token::False,
                        "null" => Token::Null,
                        "break" => Token::Break,
                        "continue" => Token::Continue,
                        "as" => Token::As,
                        "match" => Token::Match,
                        _ => Token::Identifier(ident),
                    };
                } else if ch.is_digit(10) {
                    return self.read_number();
                } else {
                    Token::Illegal
                }
            }
            None => Token::EOF,
        };

        self.read_char();
        token
    }

    fn read_identifier(&mut self) -> String {
        let position = self.position;
        while let Some(ch) = self.ch {
             if is_letter(ch) || ch.is_digit(10) {
                self.read_char();
            } else {
                break;
            }
        }
        self.input[position..self.position].to_string()
    }

    fn read_number(&mut self) -> Token {
        let position = self.position;
        while let Some(ch) = self.ch {
            if ch.is_digit(10) {
                self.read_char();
            } else {
                break;
            }
        }
        
        // Check for float (dot followed by digit, NOT ".." range operator)
        if self.ch == Some('.') && self.peek_char() != Some('.') {
             if let Some(next) = self.peek_char() {
                if next.is_digit(10) {
                    self.read_char(); // Consume dot
                    while let Some(ch) = self.ch {
                         if ch.is_digit(10) {
                            self.read_char();
                        } else {
                            break;
                        }
                    }
                     let num_str = &self.input[position..self.position];
                     return Token::Float(num_str.parse().unwrap_or(0.0));
                }
             }
        }
        
        let num_str = &self.input[position..self.position];
        Token::Integer(num_str.parse().unwrap_or(0))
    }

    fn read_string(&mut self) -> Token {
        let position = self.position + 1;
        self.read_char(); // Consume opening "
        
        loop {
            match self.ch {
                Some('"') => break,
                Some('\\') => {
                    self.read_char(); 
                    self.read_char(); 
                }
                 None => break, 
                _ => self.read_char(),
            }
        }
        
        let str_val = &self.input[position..self.position];
        // Consume closing " so next call to next_token starts fresh
        self.read_char();
        Token::String(str_val.to_string())
    }
}

fn is_letter(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let input = "let five = 5;
let ten = 10;
fn add(x, y) {
  x + y;
}
let result = add(five, ten);
";
        let mut l = Lexer::new(input);
        
        assert_eq!(l.next_token(), Token::Let);
        match l.next_token() { Token::Identifier(s) => assert_eq!(s, "five"), _ => panic!("expected ident") }
        assert_eq!(l.next_token(), Token::Equal);
        match l.next_token() { Token::Integer(n) => assert_eq!(n, 5), _ => panic!("expected int") }
        assert_eq!(l.next_token(), Token::Semicolon);
    }
    
    #[test]
    fn test_keywords() {
        let input = "let mut fn return if else while for in class struct import include pub true false null break continue as match";
        let mut l = Lexer::new(input);
        
        assert_eq!(l.next_token(), Token::Let);
        assert_eq!(l.next_token(), Token::Mut);
        assert_eq!(l.next_token(), Token::Fn);
        assert_eq!(l.next_token(), Token::Return);
        assert_eq!(l.next_token(), Token::If);
        assert_eq!(l.next_token(), Token::Else);
        assert_eq!(l.next_token(), Token::While);
        assert_eq!(l.next_token(), Token::For);
        assert_eq!(l.next_token(), Token::In);
        assert_eq!(l.next_token(), Token::Class);
        assert_eq!(l.next_token(), Token::Struct);
        assert_eq!(l.next_token(), Token::Import);
        assert_eq!(l.next_token(), Token::Include);
        assert_eq!(l.next_token(), Token::Pub);
        assert_eq!(l.next_token(), Token::True);
        assert_eq!(l.next_token(), Token::False);
        assert_eq!(l.next_token(), Token::Null);
        assert_eq!(l.next_token(), Token::Break);
        assert_eq!(l.next_token(), Token::Continue);
        assert_eq!(l.next_token(), Token::As);
        assert_eq!(l.next_token(), Token::Match);
        assert_eq!(l.next_token(), Token::EOF);
    }
    
    #[test]
    fn test_operators() {
        let input = "== != <= >= -> => .. && || ! % # .";
        let mut l = Lexer::new(input);
        
        assert_eq!(l.next_token(), Token::EqualEqual);
        assert_eq!(l.next_token(), Token::NotEqual);
        assert_eq!(l.next_token(), Token::LessEqual);
        assert_eq!(l.next_token(), Token::GreaterEqual);
        assert_eq!(l.next_token(), Token::Arrow);
        assert_eq!(l.next_token(), Token::FatArrow);
        assert_eq!(l.next_token(), Token::DotDot);
        assert_eq!(l.next_token(), Token::And);
        assert_eq!(l.next_token(), Token::Or);
        assert_eq!(l.next_token(), Token::Bang);
        assert_eq!(l.next_token(), Token::Percent);
        assert_eq!(l.next_token(), Token::Hash);
        assert_eq!(l.next_token(), Token::Dot);
        assert_eq!(l.next_token(), Token::EOF);
    }
    
    #[test]
    fn test_comments() {
        let input = "let x = 5 // this is a comment\nlet y = 10";
        let mut l = Lexer::new(input);
        
        assert_eq!(l.next_token(), Token::Let);
        match l.next_token() { Token::Identifier(s) => assert_eq!(s, "x"), _ => panic!() }
        assert_eq!(l.next_token(), Token::Equal);
        match l.next_token() { Token::Integer(n) => assert_eq!(n, 5), _ => panic!() }
        // Comment should be skipped
        assert_eq!(l.next_token(), Token::Let);
        match l.next_token() { Token::Identifier(s) => assert_eq!(s, "y"), _ => panic!() }
        assert_eq!(l.next_token(), Token::Equal);
        match l.next_token() { Token::Integer(n) => assert_eq!(n, 10), _ => panic!() }
        assert_eq!(l.next_token(), Token::EOF);
    }
    
    #[test]
    fn test_range_vs_float() {
        let input = "0..10 3.14";
        let mut l = Lexer::new(input);
        
        match l.next_token() { Token::Integer(n) => assert_eq!(n, 0), _ => panic!("expected 0") }
        assert_eq!(l.next_token(), Token::DotDot);
        match l.next_token() { Token::Integer(n) => assert_eq!(n, 10), _ => panic!("expected 10") }
        match l.next_token() { Token::Float(f) => assert!((f - 3.14).abs() < 0.001), _ => panic!("expected 3.14") }
        assert_eq!(l.next_token(), Token::EOF);
    }
}
