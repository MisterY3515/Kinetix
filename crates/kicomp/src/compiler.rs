/// KiComp Compiler: walks the AST and emits register-based bytecode.

use kinetix_language::ast::{Statement, Expression};
use crate::ir::*;
use std::collections::HashMap;

/// Current build version of the compiler/VM.
pub const CURRENT_BUILD: i64 = 3;

/// Scope for tracking local variable slots.
#[derive(Debug)]
struct Scope {
    locals: HashMap<String, u8>,
    next_register: u8,
}

impl Scope {
    fn new(start_register: u8) -> Self {
        Self {
            locals: HashMap::new(),
            next_register: start_register,
        }
    }



    fn define(&mut self, name: &str) -> u8 {
        let reg = self.next_register;
        self.locals.insert(name.to_string(), reg);
        self.next_register += 1;
        reg
    }

    fn resolve(&self, name: &str) -> Option<u8> {
        self.locals.get(name).copied()
    }
}

/// The main compiler struct.
pub struct Compiler {
    pub program: CompiledProgram,
    scopes: Vec<Scope>,
    next_temp: u8,
}

impl Compiler {
    pub fn new() -> Self {

        Self {
            program: CompiledProgram::new(),
            scopes: vec![Scope::new(0)],
            next_temp: 0,
        }
    }

    /// Compile a full program (list of statements).
    pub fn compile(&mut self, statements: &[Statement]) -> Result<&CompiledProgram, String> {

        for stmt in statements {
            self.compile_statement(stmt)?;
            if let Some(scope) = self.scopes.last() {
                self.next_temp = scope.next_register;
            }
        }
        self.current_fn().emit(Instruction::a_only(Opcode::Halt, 0));
        Ok(&self.program)
    }

    fn current_fn(&mut self) -> &mut CompiledFunction {
        &mut self.program.main
    }

    #[allow(dead_code)]
    fn current_scope(&self) -> &Scope {
        self.scopes.last().expect("no scope")
    }

    fn current_scope_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().expect("no scope")
    }

    fn alloc_register(&mut self) -> u8 {
        let r = self.next_temp;
        self.next_temp += 1;
        r
    }

    fn resolve_name(&self, name: &str) -> Option<u8> {
        for scope in self.scopes.iter().rev() {
            if let Some(reg) = scope.resolve(name) {
                return Some(reg);
            }
        }
        None
    }

    // ========== Statements ==========

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), String> {
        match stmt {
            Statement::Let { name, value, mutable, type_hint: _ } => {
                let reg = self.compile_expression(value)?;
                if self.scopes.len() == 1 {
                    // Global scope -> SetGlobal
                    let name_idx = self.current_fn().add_constant(Constant::String(name.clone()));
                    self.current_fn().emit(Instruction::ab(Opcode::SetGlobal, name_idx, reg));
                    // Do NOT define in local scope, so identifiers resolve to GetGlobal
                } else {
                    // Local scope
                    let slot = self.current_scope_mut().define(name);
                    if slot != reg {
                        self.current_fn().emit(Instruction::ab(Opcode::SetLocal, slot, reg));
                    }
                }
            }
            Statement::Function { name, parameters, body, return_type: _ } => {
                self.compile_function(name, parameters, body)?;
            }
            Statement::Return { value } => {
                if let Some(val) = value {
                    let reg = self.compile_expression(val)?;
                    self.current_fn().emit(Instruction::a_only(Opcode::Return, reg));
                } else {
                    self.current_fn().emit(Instruction::a_only(Opcode::ReturnVoid, 0));
                }
            }
            Statement::Expression { expression } => {
                self.compile_expression(expression)?;
            }
            Statement::Block { statements } => {
                self.scopes.push(Scope::new(self.next_temp));
                for s in statements {
                    self.compile_statement(s)?;
                    if let Some(scope) = self.scopes.last() {
                        self.next_temp = scope.next_register;
                    }
                }
                self.scopes.pop();
            }
            Statement::While { condition, body } => {
                self.compile_while(condition, body)?;
            }
            Statement::For { iterator, range, body } => {
                self.compile_for(iterator, range, body)?;
            }
            Statement::Include { .. } => {
                // Includes resolved at higher level
            }
            Statement::Class { .. } | Statement::Struct { .. } => {
                // Deferred to future phase
            }
            Statement::Break | Statement::Continue => {
                // Handled by loop context (placeholder)
            }
            Statement::Version { build } => {
                if *build > CURRENT_BUILD {
                    eprintln!("Warning: Script requires build {}, but you are running build {}. Some features may not work.", build, CURRENT_BUILD);
                }
            }
        }
        Ok(())
    }

    fn compile_function(
        &mut self,
        name: &str,
        parameters: &[(String, String)],
        body: &Statement,
    ) -> Result<(), String> {
        let mut func = CompiledFunction::new(name.to_string(), parameters.len() as u8);
        func.param_names = parameters.iter().map(|(n, _)| n.clone()).collect();

        // Save state
        let saved_main = std::mem::replace(&mut self.program.main, func);
        let saved_temp = self.next_temp;
        self.next_temp = 0;

        // Parameters occupy registers 0..arity
        self.scopes.push(Scope::new(0));
        for (pname, _) in parameters {
            self.current_scope_mut().define(pname);
            self.next_temp += 1;
        }

        // Compile body
        if let Statement::Block { statements } = body {
            for s in statements {
                self.compile_statement(s)?;
                if let Some(scope) = self.scopes.last() {
                    self.next_temp = scope.next_register;
                }
            }
        }

        // Implicit return void
        self.current_fn().emit(Instruction::a_only(Opcode::ReturnVoid, 0));
        self.scopes.pop();

        // Restore state
        let compiled_func = std::mem::replace(&mut self.program.main, saved_main);
        self.next_temp = saved_temp;

        let func_idx = self.program.functions.len();
        self.program.functions.push(compiled_func);

        // Store reference as global
        let name_const = self.current_fn().add_constant(Constant::String(name.to_string()));
        let reg = self.alloc_register();
        let idx_const = self.current_fn().add_constant(Constant::Function(func_idx));
        self.current_fn().emit(Instruction::ab(Opcode::LoadConst, reg, idx_const));
        self.current_fn().emit(Instruction::ab(Opcode::SetGlobal, name_const, reg));
        self.current_scope_mut().define(name);

        Ok(())
    }

    fn compile_while(&mut self, condition: &Expression, body: &Statement) -> Result<(), String> {
        let loop_start = self.current_fn().instructions.len();
        let cond_reg = self.compile_expression(condition)?;
        let jump_idx = self.current_fn().emit(Instruction::ab(Opcode::JumpIfFalse, 0, cond_reg));

        if let Statement::Block { statements } = body {
            for s in statements {
                self.compile_statement(s)?;
                if let Some(scope) = self.scopes.last() {
                    self.next_temp = scope.next_register;
                }
            }
        }

        self.current_fn().emit(Instruction::a_only(Opcode::Jump, loop_start as u8));
        let exit_pos = self.current_fn().instructions.len();
        self.current_fn().instructions[jump_idx].a = exit_pos as u8;

        Ok(())
    }

    fn compile_for(&mut self, variable: &str, iterable: &Expression, body: &Statement) -> Result<(), String> {
        let iter_reg = self.compile_expression(iterable)?;
        let idx_reg = self.alloc_register();
        let var_reg = self.current_scope_mut().define(variable);

        let zero_const = self.current_fn().add_constant(Constant::Integer(0));
        self.current_fn().emit(Instruction::ab(Opcode::LoadConst, idx_reg, zero_const));

        let loop_start = self.current_fn().instructions.len();
        self.current_fn().emit(Instruction::new(Opcode::GetIndex, var_reg, iter_reg, idx_reg));
        let jump_idx = self.current_fn().emit(Instruction::ab(Opcode::JumpIfFalse, 0, var_reg));

        if let Statement::Block { statements } = body {
            for s in statements {
                self.compile_statement(s)?;
            }
        }

        let one_const = self.current_fn().add_constant(Constant::Integer(1));
        let one_reg = self.alloc_register();
        self.current_fn().emit(Instruction::ab(Opcode::LoadConst, one_reg, one_const));
        self.current_fn().emit(Instruction::new(Opcode::Add, idx_reg, idx_reg, one_reg));
        self.current_fn().emit(Instruction::a_only(Opcode::Jump, loop_start as u8));

        let exit_pos = self.current_fn().instructions.len();
        self.current_fn().instructions[jump_idx].a = exit_pos as u8;

        Ok(())
    }

    // ========== Expressions ==========

    fn compile_expression(&mut self, expr: &Expression) -> Result<u8, String> {
        match expr {
            Expression::Integer(val) => {
                let reg = self.alloc_register();
                let idx = self.current_fn().add_constant(Constant::Integer(*val));
                self.current_fn().emit(Instruction::ab(Opcode::LoadConst, reg, idx));
                Ok(reg)
            }
            Expression::Float(val) => {
                let reg = self.alloc_register();
                let idx = self.current_fn().add_constant(Constant::Float(*val));
                self.current_fn().emit(Instruction::ab(Opcode::LoadConst, reg, idx));
                Ok(reg)
            }
            Expression::String(val) => {
                let reg = self.alloc_register();
                let idx = self.current_fn().add_constant(Constant::String(val.clone()));
                self.current_fn().emit(Instruction::ab(Opcode::LoadConst, reg, idx));
                Ok(reg)
            }
            Expression::Boolean(val) => {
                let reg = self.alloc_register();
                let opcode = if *val { Opcode::LoadTrue } else { Opcode::LoadFalse };
                self.current_fn().emit(Instruction::a_only(opcode, reg));
                Ok(reg)
            }
            Expression::Null => {
                let reg = self.alloc_register();
                self.current_fn().emit(Instruction::a_only(Opcode::LoadNull, reg));
                Ok(reg)
            }
            Expression::Identifier(name) => {
                if let Some(reg) = self.resolve_name(name) {
                    return Ok(reg);
                }
                // Global lookup
                let reg = self.alloc_register();
                let name_idx = self.current_fn().add_constant(Constant::String(name.clone()));
                self.current_fn().emit(Instruction::ab(Opcode::GetGlobal, reg, name_idx));
                Ok(reg)
            }
            Expression::Prefix { operator, right } => {
                let right_reg = self.compile_expression(right)?;
                let result = self.alloc_register();
                let opcode = match operator.as_str() {
                    "-" => Opcode::Neg,
                    "!" => Opcode::Not,
                    _ => return Err(format!("Unknown prefix operator: {}", operator)),
                };
                self.current_fn().emit(Instruction::ab(opcode, result, right_reg));
                Ok(result)
            }
            Expression::Infix { left, operator, right } => {
                let left_reg = self.compile_expression(left)?;
                let right_reg = self.compile_expression(right)?;
                let result = self.alloc_register();
                let opcode = match operator.as_str() {
                    "+" => Opcode::Add,
                    "-" => Opcode::Sub,
                    "*" => Opcode::Mul,
                    "/" => Opcode::Div,
                    "%" => Opcode::Mod,
                    "==" => Opcode::Eq,
                    "!=" => Opcode::Neq,
                    "<" => Opcode::Lt,
                    ">" => Opcode::Gt,
                    "<=" => Opcode::Lte,
                    ">=" => Opcode::Gte,
                    "&&" => Opcode::And,
                    "||" => Opcode::Or,
                    _ => return Err(format!("Unknown infix operator: {}", operator)),
                };
                self.current_fn().emit(Instruction::new(opcode, result, left_reg, right_reg));
                Ok(result)
            }
            Expression::Assign { target, value } => {
                let val_reg = self.compile_expression(value)?;
                match target.as_ref() {
                    Expression::Identifier(name) => {
                        if let Some(slot) = self.resolve_name(name) {
                            self.current_fn().emit(Instruction::ab(Opcode::SetLocal, slot, val_reg));
                        } else {
                            let name_idx = self.current_fn().add_constant(Constant::String(name.clone()));
                            self.current_fn().emit(Instruction::ab(Opcode::SetGlobal, name_idx, val_reg));
                        }
                    }
                    Expression::MemberAccess { object, member } => {
                        let obj_reg = self.compile_expression(object)?;
                        let name_idx = self.current_fn().add_constant(Constant::String(member.clone()));
                        self.current_fn().emit(Instruction::new(Opcode::SetMember, obj_reg, name_idx, val_reg));
                    }
                    Expression::Index { left, index } => {
                        let left_reg = self.compile_expression(left)?;
                        let idx_reg = self.compile_expression(index)?;
                        self.current_fn().emit(Instruction::new(Opcode::SetIndex, left_reg, idx_reg, val_reg));
                    }
                    _ => return Err("Invalid assignment target".to_string()),
                }
                Ok(val_reg)
            }
            Expression::Call { function, arguments } => {
                let orig_func_reg = self.compile_expression(function)?;
                // Copy function ref to a temp register so the Call opcode's
                // return-value write doesn't destroy the original variable.
                let call_reg = self.alloc_register();
                self.current_fn().emit(Instruction::ab(Opcode::SetLocal, call_reg, orig_func_reg));
                // Ensure arguments occupy contiguous registers call_reg+1..call_reg+N
                for (i, arg) in arguments.iter().enumerate() {
                    let expected_reg = call_reg + 1 + i as u8;
                    let arg_reg = self.compile_expression(arg)?;
                    if arg_reg != expected_reg {
                        // Reserve the expected slot if needed
                        while self.next_temp <= expected_reg {
                            self.alloc_register();
                        }
                        // Copy argument value into the expected slot
                        self.current_fn().emit(Instruction::ab(Opcode::SetLocal, expected_reg, arg_reg));
                    }
                }
                self.current_fn().emit(Instruction::ab(Opcode::Call, call_reg, arguments.len() as u8));
                Ok(call_reg)
            }
            Expression::If { condition, consequence, alternative } => {
                let cond_reg = self.compile_expression(condition)?;
                let result_reg = self.alloc_register();
                let jump_else = self.current_fn().emit(Instruction::ab(Opcode::JumpIfFalse, 0, cond_reg));

                if let Statement::Block { statements } = consequence.as_ref() {
                    for s in statements {
                        self.compile_statement(s)?;
                    }
                }

                if let Some(alt) = alternative {
                    let jump_end = self.current_fn().emit(Instruction::a_only(Opcode::Jump, 0));
                    let else_pos = self.current_fn().instructions.len();
                    self.current_fn().instructions[jump_else].a = else_pos as u8;

                    if let Statement::Block { statements } = alt.as_ref() {
                        for s in statements {
                            self.compile_statement(s)?;
                        }
                    }

                    let end_pos = self.current_fn().instructions.len();
                    self.current_fn().instructions[jump_end].a = end_pos as u8;
                } else {
                    let end_pos = self.current_fn().instructions.len();
                    self.current_fn().instructions[jump_else].a = end_pos as u8;
                }

                Ok(result_reg)
            }
            Expression::Index { left, index } => {
                let left_reg = self.compile_expression(left)?;
                let idx_reg = self.compile_expression(index)?;
                let result = self.alloc_register();
                self.current_fn().emit(Instruction::new(Opcode::GetIndex, result, left_reg, idx_reg));
                Ok(result)
            }
            Expression::MemberAccess { object, member } => {
                let obj_reg = self.compile_expression(object)?;
                let name_idx = self.current_fn().add_constant(Constant::String(member.clone()));
                let result = self.alloc_register();
                self.current_fn().emit(Instruction::new(Opcode::GetMember, result, obj_reg, name_idx));
                Ok(result)
            }
            Expression::ArrayLiteral(elements) => {
                let start_reg = self.next_temp;
                for elem in elements {
                    self.compile_expression(elem)?;
                }
                self.current_fn().emit(Instruction::ab(Opcode::MakeArray, start_reg, elements.len() as u8));
                Ok(start_reg)
            }
            Expression::FunctionLiteral { parameters, body, return_type: _ } => {
                let name = format!("<lambda_{}>", self.program.functions.len());
                self.compile_function(&name, parameters, body)?;
                let reg = self.alloc_register();
                let func_idx = self.program.functions.len() - 1;
                let idx = self.current_fn().add_constant(Constant::Function(func_idx));
                self.current_fn().emit(Instruction::ab(Opcode::LoadConst, reg, idx));
                Ok(reg)
            }
            Expression::Range { .. } | Expression::MapLiteral(_) | Expression::Match { .. } => {
                let reg = self.alloc_register();
                self.current_fn().emit(Instruction::a_only(Opcode::LoadNull, reg));
                Ok(reg)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kinetix_language::lexer::Lexer;
    use kinetix_language::parser::Parser;

    fn compile_source(src: &str) -> CompiledProgram {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program();
        assert!(parser.errors.is_empty(), "Parser errors: {:?}", parser.errors);

        let mut compiler = Compiler::new();
        compiler.compile(&program.statements).expect("Compilation failed");
        compiler.program
    }

    #[test]
    fn test_compile_let_integer() {
        let prog = compile_source("let x = 42;");
        assert!(!prog.main.instructions.is_empty());
        assert_eq!(prog.main.instructions.last().unwrap().opcode, Opcode::Halt);
        assert!(prog.main.constants.contains(&Constant::Integer(42)));
    }

    #[test]
    fn test_compile_arithmetic() {
        let prog = compile_source("let x = 1 + 2;");
        assert!(prog.main.constants.contains(&Constant::Integer(1)));
        assert!(prog.main.constants.contains(&Constant::Integer(2)));
        let has_add = prog.main.instructions.iter().any(|i| i.opcode == Opcode::Add);
        assert!(has_add, "Expected Add instruction");
    }

    #[test]
    fn test_compile_function() {
        let prog = compile_source("fn add(a: int, b: int) -> int { return a + b; }");
        assert_eq!(prog.functions.len(), 1);
        assert_eq!(prog.functions[0].name, "add");
        assert_eq!(prog.functions[0].arity, 2);
    }

    #[test]
    fn test_compile_if() {
        let prog = compile_source("let x = 10; if x > 5 { let y = 1; }");
        let has_jump = prog.main.instructions.iter().any(|i| i.opcode == Opcode::JumpIfFalse);
        assert!(has_jump, "Expected JumpIfFalse for if statement");
    }

    #[test]
    fn test_compile_while() {
        let prog = compile_source("let x = 0; while x < 10 { x = x + 1; }");
        let has_jump_back = prog.main.instructions.iter().any(|i| i.opcode == Opcode::Jump);
        let has_cond = prog.main.instructions.iter().any(|i| i.opcode == Opcode::JumpIfFalse);
        assert!(has_jump_back, "Expected Jump for while loop back-edge");
        assert!(has_cond, "Expected JumpIfFalse for while condition");
    }

    #[test]
    fn test_compile_string() {
        let prog = compile_source("let s = \"hello\";");
        assert!(prog.main.constants.contains(&Constant::String("hello".to_string())));
    }

    #[test]
    fn test_compile_boolean() {
        let prog = compile_source("let a = true; let b = false;");
        let has_true = prog.main.instructions.iter().any(|i| i.opcode == Opcode::LoadTrue);
        let has_false = prog.main.instructions.iter().any(|i| i.opcode == Opcode::LoadFalse);
        assert!(has_true);
        assert!(has_false);
    }
}
