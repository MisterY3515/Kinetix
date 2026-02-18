

#[derive(Debug)]
pub enum Statement<'a> {
    Let {
        name: String,
        mutable: bool,
        type_hint: Option<String>,
        value: Expression<'a>,
    },
    Return {
        value: Option<Expression<'a>>,
    },
    Expression {
        expression: Expression<'a>,
    },
    Block {
        statements: Vec<Statement<'a>>,
    },
    Function {
        name: String,
        parameters: Vec<(String, String)>, // (name, type)
        body: &'a Statement<'a>, // Block
        return_type: String,
    },
    While {
        condition: Expression<'a>,
        body: &'a Statement<'a>,
    },
    For {
        iterator: String,
        range: Expression<'a>,
        body: &'a Statement<'a>,
    },
    Class {
        name: String,
        parent: Option<String>,
        methods: Vec<Statement<'a>>, // Function statements
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

#[derive(Debug)]
pub enum Expression<'a> {
    Identifier(String),
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Null,
    Prefix {
        operator: String,
        right: &'a Expression<'a>,
    },
    Infix {
        left: &'a Expression<'a>,
        operator: String,
        right: &'a Expression<'a>,
    },
    If {
        condition: &'a Expression<'a>,
        consequence: &'a Statement<'a>, // Block
        alternative: Option<&'a Statement<'a>>, // Also Block
    },
    Call {
        function: &'a Expression<'a>,
        arguments: Vec<Expression<'a>>,
    },
    FunctionLiteral {
        parameters: Vec<(String, String)>,
        body: &'a Statement<'a>,
        return_type: String,
    },
    ArrayLiteral(Vec<Expression<'a>>),
    MapLiteral(Vec<(Expression<'a>, Expression<'a>)>),
    Index {
        left: &'a Expression<'a>,
        index: &'a Expression<'a>,
    },
    MemberAccess {
        object: &'a Expression<'a>,
        member: String,
    },
    Assign {
        target: &'a Expression<'a>,
        value: &'a Expression<'a>,
    },
    Match {
        value: &'a Expression<'a>,
        arms: Vec<(Expression<'a>, &'a Statement<'a>)>, 
    },
    Range {
        start: &'a Expression<'a>,
        end: &'a Expression<'a>,
    },
}

#[derive(Debug)]
pub struct Program<'a> {
    pub statements: Vec<Statement<'a>>,
}

impl<'a> Program<'a> {
    pub fn new() -> Self {
        Program { statements: vec![] }
    }
}
