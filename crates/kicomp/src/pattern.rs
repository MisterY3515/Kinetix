/// Shared match-arm pattern classification.
///
/// A match arm's pattern parses as a plain `Expression` (there is no dedicated
/// pattern AST node) -- `Some(x)` is `Call{function: Identifier("Some"), ...}`,
/// `None`/`Red`/`x`/`_` are all bare `Identifier`s, and everything else is a
/// literal. Symbol resolution, HIR lowering, and bytecode codegen each need to
/// tell these apart the same way, so the classification lives here once
/// instead of three times.
use kinetix_language::ast::Expression;

#[derive(Debug, Clone)]
pub enum ArmPattern<'a> {
    Wildcard,
    Binding(String),
    Literal(&'a Expression<'a>),
    Variant { name: String, binding: Option<String> },
}

/// Classifies a match-arm pattern expression. `is_nullary_variant` distinguishes
/// a bare identifier naming a no-payload enum variant (`None`, `Red`) from an
/// ordinary catch-all binding (`x`) -- both parse identically as a bare
/// `Expression::Identifier`, so this can't be told apart syntactically alone.
pub fn classify_pattern<'a>(pat: &'a Expression<'a>, is_nullary_variant: impl Fn(&str) -> bool) -> ArmPattern<'a> {
    match pat {
        Expression::Identifier(name) if name == "_" => ArmPattern::Wildcard,
        Expression::Identifier(name) if is_nullary_variant(name) => {
            ArmPattern::Variant { name: name.clone(), binding: None }
        }
        Expression::Identifier(name) => ArmPattern::Binding(name.clone()),
        Expression::Call { function, arguments } => {
            if let Expression::Identifier(vname) = &**function {
                let binding = arguments.first().and_then(|a| {
                    if let Expression::Identifier(b) = a { Some(b.clone()) } else { None }
                });
                ArmPattern::Variant { name: vname.clone(), binding }
            } else {
                ArmPattern::Wildcard
            }
        }
        other => ArmPattern::Literal(other),
    }
}
