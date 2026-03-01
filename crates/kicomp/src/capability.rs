/// Capability IR Enforcement Pass
/// 
/// This module provides a pre-lowering compile-time sandbox verification pass.
/// It scans the typed HIR for sensitive builtin invocations (File I/O, Network, System Info,
/// OS Execution, Thread Control) and statically verifies that the target program/module
/// has requested and been granted the required capabilities.
/// If a violation is found, compilation fails immediately.
/// 
/// Build 19: Foundation. Build 24: Full coverage on all system.* syscalls.

use crate::hir::*;
use crate::types::Type;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    FsRead,
    FsWrite,
    NetAccess,
    SysInfo,
    OsExecute,
    ThreadControl,
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Capability::FsRead => write!(f, "FsRead"),
            Capability::FsWrite => write!(f, "FsWrite"),
            Capability::NetAccess => write!(f, "NetAccess"),
            Capability::SysInfo => write!(f, "SysInfo"),
            Capability::OsExecute => write!(f, "OsExecute"),
            Capability::ThreadControl => write!(f, "ThreadControl"),
        }
    }
}

pub struct CapabilityError {
    pub message: String,
    pub line: usize,
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Line {}: Capability Violation - {}", self.line, self.message)
    }
}

pub struct CapabilityValidator {
    granted: Vec<Capability>,
}

impl CapabilityValidator {
    /// Create a new validator with granted capabilities.
    pub fn new(granted: Vec<Capability>) -> Self {
        Self { granted }
    }

    /// Run the capability enforcement pass over the HIR program.
    pub fn validate(&self, program: &HirProgram) -> Result<(), Vec<CapabilityError>> {
        let mut errors = Vec::new();
        for stmt in &program.statements {
            self.validate_stmt(stmt, &mut errors);
        }

        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    fn validate_stmt(&self, stmt: &HirStatement, errors: &mut Vec<CapabilityError>) {
        match &stmt.kind {
            HirStmtKind::Let { value, .. } => self.validate_expr(value, stmt.line, errors),
            HirStmtKind::State { value, .. } => self.validate_expr(value, stmt.line, errors),
            HirStmtKind::Computed { value, .. } => self.validate_expr(value, stmt.line, errors),
            HirStmtKind::Effect { body, .. } => self.validate_stmt(body, errors),
            HirStmtKind::Return { value: Some(v) } => self.validate_expr(v, stmt.line, errors),
            HirStmtKind::Expression { expression } => self.validate_expr(expression, stmt.line, errors),
            HirStmtKind::Block { statements } => {
                for s in statements { self.validate_stmt(s, errors); }
            }
            HirStmtKind::Function { body, .. } => self.validate_stmt(body, errors),
            HirStmtKind::Class { methods, .. } => {
                for m in methods { self.validate_stmt(m, errors); }
            }
            HirStmtKind::While { condition, body } => {
                self.validate_expr(condition, stmt.line, errors);
                self.validate_stmt(body, errors);
            }
            HirStmtKind::For { range, body, .. } => {
                self.validate_expr(range, stmt.line, errors);
                self.validate_stmt(body, errors);
            }
            _ => {}
        }
    }

    fn validate_expr(&self, expr: &HirExpression, line: usize, errors: &mut Vec<CapabilityError>) {
        match &expr.kind {
            HirExprKind::MethodCall { object, method, arguments } => {
                // Check if this is a capability-protected builtin invocation
                if let Type::Custom { name, .. } = &object.ty {
                    self.check_builtin_capability(name.as_str(), method.as_str(), line, errors);
                }
                
                self.validate_expr(object, line, errors);
                for arg in arguments {
                    self.validate_expr(arg, line, errors);
                }
            }
            HirExprKind::Call { function, arguments } => {
                // Build 24: intercept flattened system.* global calls
                if let HirExprKind::Identifier(ref name) = function.kind {
                    self.check_flattened_call(name, line, errors);
                }
                self.validate_expr(function, line, errors);
                for arg in arguments { self.validate_expr(arg, line, errors); }
            }
            HirExprKind::Infix { left, right, .. } => {
                self.validate_expr(left, line, errors);
                self.validate_expr(right, line, errors);
            }
            HirExprKind::Prefix { right, .. } => self.validate_expr(right, line, errors),
            HirExprKind::If { condition, consequence, alternative } => {
                self.validate_expr(condition, line, errors);
                self.validate_stmt(consequence, errors);
                if let Some(alt) = alternative { self.validate_stmt(alt, errors); }
            }
            HirExprKind::Assign { target, value } => {
                self.validate_expr(target, line, errors);
                self.validate_expr(value, line, errors);
            }
            HirExprKind::Index { left, index } => {
                self.validate_expr(left, line, errors);
                self.validate_expr(index, line, errors);
            }
            HirExprKind::ArrayLiteral(elems) => {
                for e in elems { self.validate_expr(e, line, errors); }
            }
            HirExprKind::StructLiteral(_, fields) => {
                for (_, f) in fields { self.validate_expr(f, line, errors); }
            }
            HirExprKind::MapLiteral(entries) => {
                for (k, v) in entries {
                    self.validate_expr(k, line, errors);
                    self.validate_expr(v, line, errors);
                }
            }
            HirExprKind::MemberAccess { object, .. } => self.validate_expr(object, line, errors),
            HirExprKind::FunctionLiteral { body, .. } => self.validate_stmt(body, errors),
            HirExprKind::Range { start, end } => {
                self.validate_expr(start, line, errors);
                self.validate_expr(end, line, errors);
            }
            HirExprKind::Match { value, arms } => {
                self.validate_expr(value, line, errors);
                for (_, body) in arms { self.validate_stmt(body, errors); }
            }
            _ => {}
        }
    }

    fn check_builtin_capability(&self, module: &str, method: &str, line: usize, errors: &mut Vec<CapabilityError>) {
        let req = match (module, method) {
            // Data IO
            ("data", "read_text" | "read_bytes" | "exists" | "list_dir" | "copy") => Some(Capability::FsRead),
            ("data", "write_text") => Some(Capability::FsWrite),
            // OS / System
            ("system" | "os", "cpu_usage" | "memory_free" | "memory_total" | "os_name" | "os_version" | "hostname" | "user_name" | "uptime") => Some(Capability::SysInfo),
            // DB
            ("db", "connect" | "query" | "execute") => Some(Capability::FsRead), // SQLite accesses FS
            ("db_conn", _) => Some(Capability::FsRead),
            // Net
            ("net", "get" | "post" | "download") => Some(Capability::NetAccess),
            _ => None,
        };

        if let Some(cap) = req {
            if !self.granted.contains(&cap) {
                errors.push(CapabilityError {
                    message: format!("Sandbox missing '{}' capability for {}.{}", cap, module, method),
                    line,
                });
            }
        }
    }

    /// Build 24: Check flattened multi-level global function calls (e.g. "system.os.name").
    /// The HIR flattens `system.os.name()` into `Call(Identifier("system.os.name"), args)`.
    fn check_flattened_call(&self, name: &str, line: usize, errors: &mut Vec<CapabilityError>) {
        let req = match name {
            // OS info queries
            "system.os.name" | "system.os.arch" | "system.os.isWindows" | "system.os.isLinux" | "system.os.isMac" => Some(Capability::SysInfo),
            // OS execution
            "system.exec" => Some(Capability::OsExecute),
            // Thread control
            "system.thread.spawn" | "system.thread.join" | "system.thread.sleep" => Some(Capability::ThreadControl),
            // Defer (scope-end RAII)
            "system.defer" => Some(Capability::ThreadControl),
            _ => None,
        };

        if let Some(cap) = req {
            if !self.granted.contains(&cap) {
                errors.push(CapabilityError {
                    message: format!("Sandbox missing '{}' capability for {}", cap, name),
                    line,
                });
            }
        }
    }
}
