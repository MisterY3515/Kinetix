use inkwell::context::Context;
use inkwell::builder::Builder;
use inkwell::module::Module;
use inkwell::passes::PassManager;
use inkwell::values::{BasicValue, BasicValueEnum, FunctionValue, PointerValue, IntValue, FloatValue};
use inkwell::types::BasicTypeEnum;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;
use std::collections::HashMap;
use std::path::Path;
use kinetix_language::ast::{Statement, Expression};
use crate::ir::Constant;

/// LLVM Code Generator
pub struct LLVMCodegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    fpm: PassManager<FunctionValue<'ctx>>,
    
    // Symbol table for current scope: variable name -> pointer to stack allocation
    variables: HashMap<String, PointerValue<'ctx>>,
    
    // Current function being compiled
    current_fn: Option<FunctionValue<'ctx>>,
}

impl<'ctx> LLVMCodegen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        let fpm = PassManager::create(&module);

        fpm.add_instruction_combining_pass();
        fpm.add_reassociate_pass();
        fpm.add_gvn_pass();
        fpm.add_cfg_simplification_pass();
        fpm.add_basic_alias_analysis_pass();
        fpm.add_promot_memory_to_register_pass();
        fpm.add_instruction_combining_pass();
        fpm.add_reassociate_pass();
        fpm.initialize();

        Self {
            context,
            module,
            builder,
            fpm,
            variables: HashMap::new(),
            current_fn: None,
        }
    }

    /// Compile a list of top-level statements
    pub fn compile(&mut self, statements: &[Statement]) -> Result<(), String> {
        // Create main function: i32 main()
        let i32_type = self.context.i32_type();
        let fn_type = i32_type.fn_type(&[], false);
        let main_fn = self.module.add_function("main", fn_type, None);
        let entry = self.context.append_basic_block(main_fn, "entry");
        self.builder.position_at_end(entry);
        
        self.current_fn = Some(main_fn);

        // Declare printf
        self.declare_printf();

        for stmt in statements {
            self.compile_statement(stmt)?;
        }

        // Return 0 from main
        self.builder.build_return(Some(&i32_type.const_int(0, false))).map_err(|e| e.to_string())?;

        // Verify module
        if let Err(e) = self.module.verify() {
             return Err(format!("LLVM Module verification failed: {}", e.to_string()));
        }

        Ok(())
    }

    fn declare_printf(&self) {
        let i32_type = self.context.i32_type();
        let str_type = self.context.i8_type().ptr_type(inkwell::AddressSpace::default());
        let printf_type = i32_type.fn_type(&[str_type.into()], true); // varargs
        self.module.add_function("printf", printf_type, None);
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), String> {
        match stmt {
            Statement::Expression(expr) => {
                self.compile_expression(expr)?;
                Ok(())
            }
            Statement::Let { name, value, .. } | Statement::Mut { name, value, .. } => {
                let init_val = self.compile_expression(value)?;
                let parent = self.current_fn.unwrap();
                let entry = parent.get_first_basic_block().unwrap();
                
                // Alloca in entry block
                let builder = self.context.create_builder();
                builder.position_at_end(entry); // Insert alloca at start
                // Move insertion point to strictly before the first instruction if present? 
                // Actually position_at_end(entry) might be after some instructions if we are already in body.
                // Better approach: create dedicated builder for entry block allocas or insert before terminator.
                // For simplicity MVP: just use current builder at current position for now, but alloca in entry is better for mem2reg.
                
                // Let's alloc in entry block start
                if let Some(first_instr) = entry.get_first_instruction() {
                   builder.position_before(&first_instr);
                } else {
                   builder.position_at_end(entry);
                }

                let alloca = builder.build_alloca(init_val.get_type(), name)
                    .map_err(|e| e.to_string())?;
                
                self.builder.build_store(alloca, init_val).map_err(|e| e.to_string())?;
                
                self.variables.insert(name.clone(), alloca);
                Ok(())
            }
            Statement::If { condition, consequence, alternative } => {
                self.compile_if(condition, consequence, alternative.as_deref())
            }
            Statement::While { condition, body } => {
                self.compile_while(condition, body)
            }
             Statement::Function { name, parameters, body, .. } => {
                 Err("Function definitions not yet implemented in MVP".to_string())
             } 
            _ => Err(format!("Statement type not supported in LLVM Backend MVP: {:?}", stmt)),
        }
    }

    fn compile_expression(&mut self, expr: &Expression) -> Result<BasicValueEnum<'ctx>, String> {
        match expr {
            Expression::Integer(i) => Ok(self.context.i64_type().const_int(*i as u64, false).into()),
            Expression::Float(f) => Ok(self.context.f64_type().const_float(*f).into()),
            Expression::Boolean(b) => Ok(self.context.bool_type().const_int(if *b { 1 } else { 0 }, false).into()),
            Expression::String(s) => {
                let s_ptr = self.builder.build_global_string_ptr(s, "str").map_err(|e| e.to_string())?;
                Ok(s_ptr.as_pointer_value().into()) // Treated as ptr
            }
            Expression::Identifier(name) => {
                match self.variables.get(name) {
                    Some(ptr) => {
                         let load = self.builder.build_load(self.context.i64_type(), *ptr, name) // Assume i64 for now? Need type tracking.
                             .map_err(|e| e.to_string())?;
                         Ok(load)
                    },
                    None => Err(format!("Undefined variable: {}", name)),
                }
            }
            Expression::Infix { left, operator, right } => {
                let l = self.compile_expression(left)?;
                let r = self.compile_expression(right)?;
                self.compile_binary_op(l, operator, r)
            }
            Expression::Call { function, arguments } => {
                if let Expression::Identifier(name) = function.as_ref() {
                    if name == "print" || name == "println" {
                        return self.compile_print(arguments, name == "println");
                    }
                }
                Err("Function calls not implemented yet".to_string())
            }
            _ => Err(format!("Expression type not supported in LLVM Backend MVP: {:?}", expr)),
        }
    }

    fn compile_binary_op(&self, l: BasicValueEnum<'ctx>, op: &str, r: BasicValueEnum<'ctx>) -> Result<BasicValueEnum<'ctx>, String> {
        // Simple heuristic: if any float, cast both to float. Else int.
        let is_float = l.is_float_value() || r.is_float_value();
        
        if is_float {
            let lhs = if l.is_int_value() { 
                self.builder.build_signed_int_to_float(l.into_int_value(), self.context.f64_type(), "cast").unwrap()
            } else { l.into_float_value() };
            let rhs = if r.is_int_value() {
                 self.builder.build_signed_int_to_float(r.into_int_value(), self.context.f64_type(), "cast").unwrap()
            } else { r.into_float_value() };

            match op {
                "+" => Ok(self.builder.build_float_add(lhs, rhs, "add").map_err(|e| e.to_string())?.into()),
                "-" => Ok(self.builder.build_float_sub(lhs, rhs, "sub").map_err(|e| e.to_string())?.into()),
                "*" => Ok(self.builder.build_float_mul(lhs, rhs, "mul").map_err(|e| e.to_string())?.into()),
                "/" => Ok(self.builder.build_float_div(lhs, rhs, "div").map_err(|e| e.to_string())?.into()),
                "<" => Ok(self.builder.build_float_compare(FloatPredicate::OLT, lhs, rhs, "lt").map_err(|e| e.to_string())?.into()),
                ">" => Ok(self.builder.build_float_compare(FloatPredicate::OGT, lhs, rhs, "gt").map_err(|e| e.to_string())?.into()),
                "==" => Ok(self.builder.build_float_compare(FloatPredicate::OEQ, lhs, rhs, "eq").map_err(|e| e.to_string())?.into()),
                _ => Err(format!("Operator {} not supported for floats", op))
            }
        } else {
             let lhs = l.into_int_value();
             let rhs = r.into_int_value();
             match op {
                "+" => Ok(self.builder.build_int_add(lhs, rhs, "add").map_err(|e| e.to_string())?.into()),
                "-" => Ok(self.builder.build_int_sub(lhs, rhs, "sub").map_err(|e| e.to_string())?.into()),
                "*" => Ok(self.builder.build_int_mul(lhs, rhs, "mul").map_err(|e| e.to_string())?.into()),
                "/" => Ok(self.builder.build_int_signed_div(lhs, rhs, "div").map_err(|e| e.to_string())?.into()),
                "<" => Ok(self.builder.build_int_compare(IntPredicate::SLT, lhs, rhs, "lt").map_err(|e| e.to_string())?.into()),
                ">" => Ok(self.builder.build_int_compare(IntPredicate::SGT, lhs, rhs, "gt").map_err(|e| e.to_string())?.into()),
                "==" => Ok(self.builder.build_int_compare(IntPredicate::EQ, lhs, rhs, "eq").map_err(|e| e.to_string())?.into()),
                _ => Err(format!("Operator {} not supported for ints", op))
             }
        }
    }

    fn compile_print(&self, args: &[Expression], is_ln: bool) -> Result<BasicValueEnum<'ctx>, String> {
         let printf_fn = self.module.get_function("printf").unwrap();
         
         // Build format string
         // For now, support one arg. Muti-arg requires concat or multiple calls.
         // MVP: only first arg.
         
         // This is tricky. printf expects C-types.
         // Let's implement minimal logic: if Int -> %ld, Float -> %f, String -> %s
         
         // Just a placeholder implementation for now
         Ok(self.context.i32_type().const_int(0, false).into())
    }

    fn compile_if(&mut self, cond: &Expression, cons: &Statement, alt: Option<&Statement>) -> Result<(), String> {
        let cond_val = self.compile_expression(cond)?;
        let cond_bool = if cond_val.is_int_value() { // bools are i1
             cond_val.into_int_value()
        } else {
            return Err("Condition must be boolean/integer".to_string()); 
        };

        let parent = self.current_fn.unwrap();
        
        let then_bb = self.context.append_basic_block(parent, "then");
        let else_bb = self.context.append_basic_block(parent, "else"); // needed even if empty? Not strictly.
        let merge_bb = self.context.append_basic_block(parent, "ifcont");

        self.builder.build_conditional_branch(cond_bool, then_bb, else_bb).map_err(|e| e.to_string())?;

        // Then
        self.builder.position_at_end(then_bb);
        self.compile_statement(cons)?;
        self.builder.build_unconditional_branch(merge_bb).map_err(|e| e.to_string())?;

        // Else
        self.builder.position_at_end(else_bb);
        if let Some(alt_stmt) = alt {
             self.compile_statement(alt_stmt)?;
        }
        self.builder.build_unconditional_branch(merge_bb).map_err(|e| e.to_string())?;

        // Merge
        self.builder.position_at_end(merge_bb);
        Ok(())
    }

    fn compile_while(&mut self, cond: &Expression, body: &Statement) -> Result<(), String> {
        let parent = self.current_fn.unwrap();
        let loop_bb = self.context.append_basic_block(parent, "loop");
        let body_bb = self.context.append_basic_block(parent, "loop_body");
        let after_bb = self.context.append_basic_block(parent, "after_loop");

        // Jump to loop condition check
        self.builder.build_unconditional_branch(loop_bb).map_err(|e| e.to_string())?;

        // Loop header (condition)
        self.builder.position_at_end(loop_bb);
        let cond_val = self.compile_expression(cond)?;
         let cond_bool = if cond_val.is_int_value() {
             cond_val.into_int_value()
        } else {
            return Err("Condition must be boolean/integer".to_string()); 
        };
        self.builder.build_conditional_branch(cond_bool, body_bb, after_bb).map_err(|e| e.to_string())?;

        // Loop body
        self.builder.position_at_end(body_bb);
        self.compile_statement(body)?;
        self.builder.build_unconditional_branch(loop_bb).map_err(|e| e.to_string())?;

        // After loop
        self.builder.position_at_end(after_bb);
        Ok(())
    }

    pub fn emit_object(&self, path: &Path) -> Result<(), String> {
        use inkwell::targets::{Target, InitializationConfig};

        Target::initialize_native(&InitializationConfig::default()).map_err(|e| e.to_string())?;
        
        let triple = Target::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
        let machine = target.create_target_machine(
            &triple,
            "generic",
            "",
            inkwell::OptimizationLevel::Default,
            inkwell::targets::RelocMode::Default,
            inkwell::targets::CodeModel::Default
        ).ok_or("Could not create target machine")?;

        self.module.set_data_layout(&machine.get_target_data().get_data_layout());
        self.module.set_triple(&triple);

        machine.write_to_file(&self.module, inkwell::targets::FileType::Object, path)
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn run_jit(&self) -> Result<i32, String> {
        inkwell::targets::Target::initialize_native(&inkwell::targets::InitializationConfig::default())
            .map_err(|e| e.to_string())?;

        let execution_engine = self.module.create_jit_execution_engine(inkwell::OptimizationLevel::Default)
            .map_err(|e| e.to_string())?;

        type MainFunc = unsafe extern "C" fn() -> i32;

        unsafe {
            let main = execution_engine.get_function::<MainFunc>("main")
                .map_err(|e| e.to_string())?;
            
            Ok(main.call())
        }
    }
}

/// Convenience function to compile a program to an object file
pub fn compile_program_to_object(statements: &[Statement], output_path: &Path) -> Result<(), String> {
    let context = Context::create();
    let mut codegen = LLVMCodegen::new(&context, "main");
    codegen.compile(statements)?;
    codegen.emit_object(output_path)
}

/// Convenience function to run JIT
pub fn run_program_jit(statements: &[Statement]) -> Result<i32, String> {
    let context = Context::create();
    let mut codegen = LLVMCodegen::new(&context, "main");
    codegen.compile(statements)?;
    codegen.run_jit()
}
