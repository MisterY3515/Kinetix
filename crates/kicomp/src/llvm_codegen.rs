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

        // Declare stdlib functions (printf, malloc, strings, math)
        self.declare_stdlib();

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

    fn declare_stdlib(&self) {
        let i32_type = self.context.i32_type();
        let i64_type = self.context.i64_type();
        let f64_type = self.context.f64_type();
        let i8_ptr_type = self.context.i8_type().ptr_type(inkwell::AddressSpace::default());

        // libc IO
        let printf_type = i32_type.fn_type(&[i8_ptr_type.into()], true);
        self.module.add_function("printf", printf_type, None);

        // libc Memory
        let malloc_type = i8_ptr_type.fn_type(&[i64_type.into()], false);
        self.module.add_function("malloc", malloc_type, None);

        let strcmp_type = i32_type.fn_type(&[i8_ptr_type.into(), i8_ptr_type.into()], false);
        self.module.add_function("strcmp", strcmp_type, None);

        let memcpy_type = i8_ptr_type.fn_type(&[i8_ptr_type.into(), i8_ptr_type.into(), i64_type.into()], false);
        self.module.add_function("memcpy", memcpy_type, None);

        // libm Math
        let math_fn_type = f64_type.fn_type(&[f64_type.into()], false);
        self.module.add_function("sin", math_fn_type, None);
        self.module.add_function("cos", math_fn_type, None);
        self.module.add_function("sqrt", math_fn_type, None);
        let math_fn_2_type = f64_type.fn_type(&[f64_type.into(), f64_type.into()], false);
        self.module.add_function("pow", math_fn_2_type, None);

        // Struct Types
        let string_struct = self.context.opaque_struct_type("String");
        string_struct.set_body(&[i64_type.into(), i8_ptr_type.into()], false);

        let array_struct = self.context.opaque_struct_type("Array");
        // { i64 len, i64 cap, i64* data } - simplistic array of integers for naive LLVM execution
        array_struct.set_body(&[i64_type.into(), i64_type.into(), i64_type.ptr_type(inkwell::AddressSpace::default()).into()], false);
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), String> {
        match stmt {
            Statement::Expression { expression, .. } => {
                self.compile_expression(expression)?;
                Ok(())
            }
            Statement::Let { name, value, .. } => {
                let init_val = self.compile_expression(value)?;
                let parent = self.current_fn.unwrap();
                let entry = parent.get_first_basic_block().unwrap();
                
                // Alloca in entry block
                let builder = self.context.create_builder();
                builder.position_at_end(entry);
                
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
            Statement::While { condition, body, .. } => {
                self.compile_while(condition, body)
            }
            Statement::Function { name, parameters, body, .. } => {
                self.compile_fn_def(name, parameters, body)
            }
            Statement::Return { value, .. } => {
                if let Some(val_expr) = value {
                    let val = self.compile_expression(val_expr)?;
                    self.builder.build_return(Some(&val)).map_err(|e| e.to_string())?;
                } else {
                    self.builder.build_return(None).map_err(|e| e.to_string())?;
                }
                Ok(())
            }
            Statement::Block { statements, .. } => {
                for s in statements {
                    self.compile_statement(s)?;
                }
                Ok(())
            }
            Statement::For { .. } => {
                // For loops need iterator support — deferred
                Ok(())
            }
            _ => Ok(()), // Silently skip unsupported statements (Include, Class, Struct, etc.)
        }
    }

    /// Compile a user-defined function definition into LLVM IR.
    fn compile_fn_def(&mut self, name: &str, parameters: &[(String, String)], body: &Statement) -> Result<(), String> {
        let i64_type = self.context.i64_type();

        // Build function type: all params are i64, returns i64 (MVP simplification)
        let param_types: Vec<inkwell::types::BasicMetadataTypeEnum> = parameters.iter()
            .map(|_| i64_type.into())
            .collect();
        let fn_type = i64_type.fn_type(&param_types, false);
        let function = self.module.add_function(name, fn_type, None);

        // Save current compilation state
        let saved_fn = self.current_fn;
        let saved_vars = self.variables.clone();

        self.current_fn = Some(function);
        self.variables.clear();

        // Create entry block
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        // Create allocas for parameters
        let alloca_builder = self.context.create_builder();
        alloca_builder.position_at_end(entry);
        for (i, (param_name, _)) in parameters.iter().enumerate() {
            let alloca = alloca_builder.build_alloca(i64_type, param_name)
                .map_err(|e| e.to_string())?;
            let param_val = function.get_nth_param(i as u32)
                .ok_or_else(|| format!("Missing parameter {}", i))?;
            self.builder.build_store(alloca, param_val).map_err(|e| e.to_string())?;
            self.variables.insert(param_name.clone(), alloca);
        }

        // Compile the body
        self.compile_statement(body)?;

        // Add implicit return 0 if no terminator
        let current_bb = self.builder.get_insert_block().unwrap();
        if current_bb.get_terminator().is_none() {
            self.builder.build_return(Some(&i64_type.const_int(0, false)))
                .map_err(|e| e.to_string())?;
        }

        // Run function pass manager
        self.fpm.run_on(&function);

        // Restore state
        self.current_fn = saved_fn;
        self.variables = saved_vars;

        // Position builder back in the caller (main function)
        if let Some(parent) = saved_fn {
            if let Some(last_bb) = parent.get_last_basic_block() {
                self.builder.position_at_end(last_bb);
            }
        }

        Ok(())
    }


    fn compile_expression(&mut self, expr: &Expression) -> Result<BasicValueEnum<'ctx>, String> {
        match expr {
            Expression::Try { value } => self.compile_expression(value), // TEMPORARY stub
            Expression::Integer(i) => Ok(self.context.i64_type().const_int(*i as u64, false).into()),
            Expression::Float(f) => Ok(self.context.f64_type().const_float(*f).into()),
            Expression::Boolean(b) => Ok(self.context.bool_type().const_int(if *b { 1 } else { 0 }, false).into()),
            Expression::String(s) => {
                let string_type = self.context.get_struct_type("String").unwrap();
                let i64_type = self.context.i64_type();
                let mut str_struct = string_type.get_undef();
                
                let s_ptr = self.builder.build_global_string_ptr(s, "str_data").map_err(|e| e.to_string())?;
                let len_val = i64_type.const_int(s.len() as u64, false);
                
                str_struct = self.builder.build_insert_value(str_struct, len_val, 0, "insert_len")
                    .map_err(|e| e.to_string())?
                    .into_struct_value();
                str_struct = self.builder.build_insert_value(str_struct, s_ptr.as_pointer_value(), 1, "insert_ptr")
                    .map_err(|e| e.to_string())?
                    .into_struct_value();
                
                Ok(str_struct.into())
            }
            Expression::ArrayLiteral(elements) => {
                let array_type = self.context.get_struct_type("Array").unwrap();
                let i64_type = self.context.i64_type();
                let mut arr_struct = array_type.get_undef();
                
                let len = elements.len() as u64;
                let len_val = i64_type.const_int(len, false);
                
                let malloc_fn = self.module.get_function("malloc").unwrap();
                let bytes_val = i64_type.const_int(len * 8, false);
                let data_ptr = self.builder.build_call(malloc_fn, &[bytes_val.into()], "malloc_array")
                    .map_err(|e| e.to_string())?
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                
                let i64_ptr_type = i64_type.ptr_type(inkwell::AddressSpace::default());
                let data_i64_ptr = self.builder.build_pointer_cast(data_ptr.into_pointer_value(), i64_ptr_type, "cast").unwrap();
                
                for (i, elem) in elements.iter().enumerate() {
                    let elem_val = self.compile_expression(elem)?;
                    let idx_val = i64_type.const_int(i as u64, false);
                    let ptr = unsafe { self.builder.build_gep(i64_type, data_i64_ptr, &[idx_val], "elem_ptr") }
                        .map_err(|e| e.to_string())?;
                    // For MVP simplicity, we forcefully cast floats to bits or just store integers
                    let to_store = if elem_val.is_int_value() {
                        elem_val.into_int_value()
                    } else if elem_val.is_float_value() {
                        // For simplicity in array storing, cast float to bitwise i64
                        self.builder.build_bitcast(elem_val.into_float_value(), i64_type, "bitcast_float").unwrap().into_int_value()
                    } else {
                        // Struct or other? Just store 0 for now as dummy.
                        i64_type.const_int(0, false)
                    };
                    self.builder.build_store(ptr, to_store).map_err(|e| e.to_string())?;
                }
                
                arr_struct = self.builder.build_insert_value(arr_struct, len_val, 0, "insert_len").unwrap().into_struct_value();
                arr_struct = self.builder.build_insert_value(arr_struct, len_val, 1, "insert_cap").unwrap().into_struct_value();
                arr_struct = self.builder.build_insert_value(arr_struct, data_i64_ptr, 2, "insert_data").unwrap().into_struct_value();
                
                Ok(arr_struct.into())
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
                    if name.starts_with("math.") {
                        let fn_name = name.strip_prefix("math.").unwrap();
                        match fn_name {
                            "sin" | "cos" | "sqrt" => {
                                if arguments.len() != 1 { return Err(format!("{} requires 1 argument", name)); }
                                let arg = self.compile_expression(&arguments[0])?;
                                let arg_f = if arg.is_int_value() {
                                    self.builder.build_signed_int_to_float(arg.into_int_value(), self.context.f64_type(), "cast").unwrap()
                                } else { arg.into_float_value() };
                                let f = self.module.get_function(fn_name).unwrap();
                                let ret = self.builder.build_call(f, &[arg_f.into()], "math").unwrap().try_as_basic_value().left().unwrap();
                                return Ok(ret);
                            }
                            "pow" => {
                                if arguments.len() != 2 { return Err("pow requires 2 arguments".to_string()); }
                                let a1 = self.compile_expression(&arguments[0])?;
                                let a2 = self.compile_expression(&arguments[1])?;
                                let f1 = if a1.is_int_value() { self.builder.build_signed_int_to_float(a1.into_int_value(), self.context.f64_type(), "c").unwrap() } else { a1.into_float_value() };
                                let f2 = if a2.is_int_value() { self.builder.build_signed_int_to_float(a2.into_int_value(), self.context.f64_type(), "c").unwrap() } else { a2.into_float_value() };
                                let f = self.module.get_function("pow").unwrap();
                                let ret = self.builder.build_call(f, &[f1.into(), f2.into()], "pow").unwrap().try_as_basic_value().left().unwrap();
                                return Ok(ret);
                            }
                            _ => {}
                        }
                    }

                    // Try user-defined function
                    if let Some(func) = self.module.get_function(name) {
                        let mut args: Vec<inkwell::values::BasicMetadataValueEnum> = Vec::new();
                        for arg_expr in arguments {
                            let val = self.compile_expression(arg_expr)?;
                            args.push(val.into());
                        }
                        let call = self.builder.build_call(func, &args, &format!("call_{}", name))
                            .map_err(|e| e.to_string())?;
                        return match call.try_as_basic_value().left() {
                            Some(v) => Ok(v),
                            None => Ok(self.context.i64_type().const_int(0, false).into()),
                        };
                    }
                }
                Err(format!("Cannot resolve function call: {:?}", function))
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
