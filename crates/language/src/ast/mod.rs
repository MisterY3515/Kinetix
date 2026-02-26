

#[derive(Debug)]
pub enum Statement<'a> {
    Let {
        name: String,
        mutable: bool,
        type_hint: Option<String>,
        value: Expression<'a>,
        line: usize,
    },
    State {
        name: String,
        type_hint: Option<String>,
        value: Expression<'a>,
        line: usize,
    },
    Computed {
        name: String,
        type_hint: Option<String>,
        value: Expression<'a>,
        line: usize,
    },
    Effect {
        dependencies: Vec<String>,
        body: &'a Statement<'a>,
        line: usize,
    },
    Return {
        value: Option<Expression<'a>>,
        line: usize,
    },
    Expression {
        expression: Expression<'a>,
        line: usize,
    },
    Block {
        statements: Vec<Statement<'a>>,
        line: usize,
    },
    Function {
        name: String,
        parameters: Vec<(String, String)>, // (name, type)
        body: &'a Statement<'a>, // Block
        return_type: String,
        line: usize,
    },
    While {
        condition: Expression<'a>,
        body: &'a Statement<'a>,
        line: usize,
    },
    For {
        iterator: String,
        range: Expression<'a>,
        body: &'a Statement<'a>,
        line: usize,
    },
    Class {
        name: String,
        parent: Option<String>,
        methods: Vec<Statement<'a>>, // Function statements
        fields: Vec<(bool, String, String)>, // (is_public, name, type)
        line: usize,
    },
    Struct {
        name: String,
        fields: Vec<(String, String)>,
        line: usize,
    },
    Include {
        path: String,
        alias: Option<String>,
        line: usize,
    },
    Version {
        build: i64,
        line: usize,
    },
    Enum {
        name: String,
        generics: Vec<String>,
        variants: Vec<(String, Option<String>)>, // VariantName(OptionalPayloadType)
        line: usize,
    },
    Trait {
        name: String,
        generics: Vec<String>,
        methods: Vec<(String, Vec<(String, String)>, String)>, // MethodName, Params, ReturnType
        line: usize,
    },
    Impl {
        trait_name: Option<String>,
        target_name: String,
        generics: Vec<String>,
        methods: Vec<Statement<'a>>, // Functions
        line: usize,
    },
    Break { line: usize },
    Continue { line: usize },
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
    StructLiteral {
        name: String,
        fields: Vec<(String, Expression<'a>)>,
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
    Try {
        value: &'a Expression<'a>,
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
