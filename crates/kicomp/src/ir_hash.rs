use std::hash::Hasher;

/// A deterministic FNV-1a 64-bit hasher.
/// `DefaultHasher` from `std::collections` uses a random seed per process,
/// which breaks deterministic compile-time hashing across different compiler runs.
pub struct DeterministicHasher {
    hash: u64,
}

impl DeterministicHasher {
    pub fn new() -> Self {
        Self {
            hash: 0xcbf29ce484222325, // FNV-1a 64-bit offset basis
        }
    }
}

impl Default for DeterministicHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for DeterministicHasher {
    fn finish(&self) -> u64 {
        self.hash
    }

    fn write(&mut self, bytes: &[u8]) {
        let prime: u64 = 0x100000001b3; // FNV-1a 64-bit prime
        for &byte in bytes {
            self.hash ^= byte as u64;
            self.hash = self.hash.wrapping_mul(prime);
        }
    }
}

/// Compute a deterministic structural hash of the entire HIR program.
///
/// Uses the FNV-1a hasher and walks all statements/expressions/types
/// to produce a single u64 fingerprint.
/// Identical source code will always produce the same hash across compiler runs.
pub fn hash_hir_program(program: &crate::hir::HirProgram) -> u64 {
    use crate::hir::*;

    let mut hasher = DeterministicHasher::new();

    fn hash_type(ty: &crate::types::Type, h: &mut DeterministicHasher) {
        use std::hash::Hash;
        ty.hash(h);
    }

    fn hash_expr(expr: &HirExpression, h: &mut DeterministicHasher) {
        hash_type(&expr.ty, h);
        match &expr.kind {
            HirExprKind::Identifier(name) => { h.write(b"id"); h.write(name.as_bytes()); }
            HirExprKind::Integer(v) => { h.write(b"int"); h.write(&v.to_le_bytes()); }
            HirExprKind::Float(v) => { h.write(b"flt"); h.write(&v.to_bits().to_le_bytes()); }
            HirExprKind::String(s) => { h.write(b"str"); h.write(s.as_bytes()); }
            HirExprKind::Boolean(b) => { h.write(b"bool"); h.write(&[*b as u8]); }
            HirExprKind::Null => { h.write(b"null"); }
            HirExprKind::Prefix { operator, right } => {
                h.write(b"pre"); h.write(operator.as_bytes());
                hash_expr(right, h);
            }
            HirExprKind::Infix { left, operator, right } => {
                h.write(b"inf"); h.write(operator.as_bytes());
                hash_expr(left, h); hash_expr(right, h);
            }
            HirExprKind::If { condition, consequence, alternative } => {
                h.write(b"if");
                hash_expr(condition, h); hash_stmt(consequence, h);
                if let Some(alt) = alternative { hash_stmt(alt, h); }
            }
            HirExprKind::Call { function, arguments } => {
                h.write(b"call");
                hash_expr(function, h);
                for a in arguments { hash_expr(a, h); }
            }
            HirExprKind::FunctionLiteral { parameters, body, return_type } => {
                h.write(b"fnlit");
                for (n, t) in parameters { h.write(n.as_bytes()); hash_type(t, h); }
                hash_stmt(body, h);
                hash_type(return_type, h);
            }
            HirExprKind::ArrayLiteral(elems) => {
                h.write(b"arr");
                for e in elems { hash_expr(e, h); }
            }
            HirExprKind::StructLiteral(name, fields) => {
                h.write(b"struct"); h.write(name.as_bytes());
                for (fname, fval) in fields { h.write(fname.as_bytes()); hash_expr(fval, h); }
            }
            HirExprKind::MapLiteral(entries) => {
                h.write(b"map");
                for (k, v) in entries { hash_expr(k, h); hash_expr(v, h); }
            }
            HirExprKind::Index { left, index } => {
                h.write(b"idx"); hash_expr(left, h); hash_expr(index, h);
            }
            HirExprKind::MethodCall { object, method, arguments } => {
                h.write(b"mcall"); h.write(method.as_bytes());
                hash_expr(object, h);
                for a in arguments { hash_expr(a, h); }
            }
            HirExprKind::MemberAccess { object, member } => {
                h.write(b"mem"); h.write(member.as_bytes());
                hash_expr(object, h);
            }
            HirExprKind::Assign { target, value } => {
                h.write(b"asgn"); hash_expr(target, h); hash_expr(value, h);
            }
            HirExprKind::Range { start, end } => {
                h.write(b"rng"); hash_expr(start, h); hash_expr(end, h);
            }
            HirExprKind::Match { value, arms } => {
                h.write(b"match"); hash_expr(value, h);
                for (pat, body) in arms { hash_pattern(pat, h); hash_stmt(body, h); }
            }
        }
    }

    fn hash_pattern(pat: &HirPattern, h: &mut DeterministicHasher) {
        match pat {
            HirPattern::Literal(e) => { h.write(b"plit"); hash_expr(e, h); }
            HirPattern::Variant { name, binding } => {
                h.write(b"pvar"); h.write(name.as_bytes());
                if let Some(b) = binding { h.write(b.as_bytes()); }
            }
            HirPattern::Wildcard => { h.write(b"pwild"); }
            HirPattern::Binding(name) => { h.write(b"pbind"); h.write(name.as_bytes()); }
        }
    }

    fn hash_stmt(stmt: &HirStatement, h: &mut DeterministicHasher) {
        hash_type(&stmt.ty, h);
        h.write(&stmt.line.to_le_bytes());
        match &stmt.kind {
            HirStmtKind::Let { name, mutable, value } => {
                h.write(b"let"); h.write(name.as_bytes()); h.write(&[*mutable as u8]);
                hash_expr(value, h);
            }
            HirStmtKind::State { name, value } => {
                h.write(b"state"); h.write(name.as_bytes()); hash_expr(value, h);
            }
            HirStmtKind::Computed { name, value } => {
                h.write(b"computed"); h.write(name.as_bytes()); hash_expr(value, h);
            }
            HirStmtKind::Effect { dependencies, body } => {
                h.write(b"effect");
                for d in dependencies { h.write(d.as_bytes()); }
                hash_stmt(body, h);
            }
            HirStmtKind::Return { value } => {
                h.write(b"ret");
                if let Some(v) = value { hash_expr(v, h); }
            }
            HirStmtKind::Expression { expression } => {
                h.write(b"expr"); hash_expr(expression, h);
            }
            HirStmtKind::Block { statements } => {
                h.write(b"block");
                for s in statements { hash_stmt(s, h); }
            }
            HirStmtKind::Class { name, methods } => {
                h.write(b"class"); h.write(name.as_bytes());
                for m in methods { hash_stmt(m, h); }
            }
            HirStmtKind::Function { name, parameters, body, return_type } => {
                h.write(b"fn"); h.write(name.as_bytes());
                for (pn, pt) in parameters { h.write(pn.as_bytes()); hash_type(pt, h); }
                hash_stmt(body, h);
                hash_type(return_type, h);
            }
            HirStmtKind::While { condition, body } => {
                h.write(b"while"); hash_expr(condition, h); hash_stmt(body, h);
            }
            HirStmtKind::For { iterator, range, body } => {
                h.write(b"for"); h.write(iterator.as_bytes());
                hash_expr(range, h); hash_stmt(body, h);
            }
            HirStmtKind::Break => { h.write(b"break"); }
            HirStmtKind::Continue => { h.write(b"continue"); }
        }
    }

    for stmt in &program.statements {
        hash_stmt(stmt, &mut hasher);
    }

    hasher.finish()
}
