//! Unused macro definition detection.
//!
//! A macro defined via `-define(M, ...)` but never referenced via `?M`,
//! `-ifdef(M)`, or `-ifndef(M)` is dead code.
//!
//! Unlike record/type warnings, macro usage is tracked at the preprocessor
//! level (before the AST exists) and passed in as a `HashSet<String>`.

use std::collections::HashSet;

use nera_syntax::error::Diagnostic;
use nera_syntax::preproc::MacroLocation;

/// Check for macros that are defined but never used.
///
/// `macro_locations` contains all `-define(M, ...)` definitions found by the
/// preprocessor. `used_macros` contains the names of macros that were actually
/// referenced (via `?M`, `-ifdef(M)`, or `-ifndef(M)`).
pub fn check_unused_macros(
    macro_locations: &[MacroLocation],
    used_macros: &HashSet<String>,
    warnings: &mut Vec<Diagnostic>,
) {
    for loc in macro_locations {
        if !used_macros.contains(&loc.name) {
            warnings.push(
                Diagnostic::warning_at(
                    format!("macro '{}' is unused", loc.name),
                    loc.combined_span,
                )
                .with_code("unused_macro"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nera_syntax::span::Span;

    fn make_loc(name: &str) -> MacroLocation {
        MacroLocation {
            name: name.to_string(),
            arity: None,
            combined_span: Span::new(0, 1),
        }
    }

    fn unused_macro_warnings(warnings: &[Diagnostic]) -> Vec<&Diagnostic> {
        warnings.iter().filter(|w| w.code.as_deref() == Some("unused_macro")).collect()
    }

    #[test]
    fn unused_macro_warns() {
        let locs = vec![make_loc("FOO")];
        let used = HashSet::new();
        let mut ws = Vec::new();
        check_unused_macros(&locs, &used, &mut ws);
        let uw = unused_macro_warnings(&ws);
        assert_eq!(uw.len(), 1);
        assert!(uw[0].message.contains("'FOO'"));
    }

    #[test]
    fn used_macro_no_warning() {
        let locs = vec![make_loc("FOO")];
        let used: HashSet<String> = ["FOO".to_string()].into();
        let mut ws = Vec::new();
        check_unused_macros(&locs, &used, &mut ws);
        assert!(unused_macro_warnings(&ws).is_empty());
    }

    #[test]
    fn multiple_macros_partial_use() {
        let locs = vec![make_loc("USED"), make_loc("UNUSED1"), make_loc("UNUSED2")];
        let used: HashSet<String> = ["USED".to_string()].into();
        let mut ws = Vec::new();
        check_unused_macros(&locs, &used, &mut ws);
        let uw = unused_macro_warnings(&ws);
        assert_eq!(uw.len(), 2);
        let names: Vec<&str> = uw.iter().map(|w| w.message.as_str()).collect();
        assert!(names.iter().any(|m| m.contains("'UNUSED1'")));
        assert!(names.iter().any(|m| m.contains("'UNUSED2'")));
    }

    #[test]
    fn all_macros_used_no_warning() {
        let locs = vec![make_loc("A"), make_loc("B")];
        let used: HashSet<String> = ["A".to_string(), "B".to_string()].into();
        let mut ws = Vec::new();
        check_unused_macros(&locs, &used, &mut ws);
        assert!(unused_macro_warnings(&ws).is_empty());
    }

    #[test]
    fn no_macros_no_warning() {
        let locs: Vec<MacroLocation> = vec![];
        let used = HashSet::new();
        let mut ws = Vec::new();
        check_unused_macros(&locs, &used, &mut ws);
        assert!(unused_macro_warnings(&ws).is_empty());
    }
}
