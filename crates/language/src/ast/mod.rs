
#[derive(Debug, PartialEq, Clone)]
pub enum Statement {
    Let {
        name: String,
        mutable: bool,
        type_hint: Option<String>,
        value: Expression,
    },
    Return {
        value: Option<Expression>,
    },
    Expression {
        expression: Expression,
    },
    Block {
        statements: Vec<Statement>,
    },
    Function {
        name: String,
        parameters: Vec<(String, String)>, // (name, type)
        body: Box<Statement>, // Block
        return_type: String,
    },
    While {
        condition: Expression,
        body: Box<Statement>,
    },
    For {
        iterator: String,
        range: Expression,
        body: Box<Statement>,
    },
    Class {
        name: String,
        parent: Option<String>,
        methods: Vec<Statement>, // Function statements
        fields: Vec<(bool, String, String)>, // (is_public, name, type)
    },
    Struct {
        name: String,
        fields: Vec<(String, String)>,
    },
    Include {
        path: String,
        alias: Option<String>,
    },
    Version {
        build: i64,
    },
    Break,
    Continue,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Expression {
    Identifier(String),
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Null,
    Prefix {
        operator: String,
        right: Box<Expression>,
    },
    Infix {
        left: Box<Expression>,
        operator: String,
        right: Box<Expression>,
    },
    If {
        condition: Box<Expression>,
        consequence: Box<Statement>, // Block
        alternative: Option<Box<Statement>>, // Also Block
    },
    Call {
        function: Box<Expression>,
        arguments: Vec<Expression>,
    },
    FunctionLiteral {
        parameters: Vec<(String, String)>,
        body: Box<Statement>,
        return_type: String,
    },
    ArrayLiteral(Vec<Expression>),
    MapLiteral(Vec<(Expression, Expression)>),
    Index {
        left: Box<Expression>,
        index: Box<Expression>,
    },
    MemberAccess {
        object: Box<Expression>,
        member: String,
    },
    Assign {
        target: Box<Expression>,
        value: Box<Expression>,
    },
    Match {
        value: Box<Expression>,
        arms: Vec<(Expression, Box<Statement>)>, 
    },
    Range {
        start: Box<Expression>,
        end: Box<Expression>,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub struct Program {
    pub statements: Vec<Statement>,
}

impl Program {
    pub fn new() -> Self {
        Program { statements: vec![] }
    }
}
