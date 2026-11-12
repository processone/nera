//! Comment-based pragma system for suppressing warnings.
//!
//! Scans raw source text for `@nera:nowarn(code)` pragmas in comments
//! preceding each function.  These pragmas suppress warnings with the
//! matching diagnostic code for the entire function.
//!
//! Syntax (in an Erlang comment):
//! ```erlang
//! %% @nera:nowarn(non_tail_recursion)
//! fib(0) -> 1;
//! fib(N) -> fib(N-1) + fib(N-2).
//! ```
//!
//! Multiple pragmas can appear on separate lines before the same function.
//! The system is generic: any warning code can be suppressed.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast;
use nera_syntax::error::Diagnostic;

/// Extract `@nera:nowarn` pragmas from source comments preceding each function.
///
/// Returns a map: `function span start → set of suppressed warning codes`.
pub(super) fn collect_function_pragmas(
    module: &ast::Module,
    source: &str,
) -> HashMap<usize, HashSet<String>> {
    let mut result: HashMap<usize, HashSet<String>> = HashMap::new();

    let mut prev_end = 0;
    for func in &module.functions {
        let preamble = &source[prev_end..func.span.start.min(source.len())];
        let codes = extract_nowarn_codes(preamble);
        if !codes.is_empty() {
            result.insert(func.span.start, codes);
        }
        prev_end = func.span.end.min(source.len());
    }

    result
}

/// Filter out warnings that are suppressed by pragmas.
///
/// For each warning, finds the enclosing function by span containment and
/// checks whether its code appears in the pragma set for that function.
pub(super) fn filter_suppressed(
    warnings: &mut Vec<Diagnostic>,
    module: &ast::Module,
    suppressed: &HashMap<usize, HashSet<String>>,
) {
    if suppressed.is_empty() {
        return;
    }
    warnings.retain(|w| {
        let code = match &w.code {
            Some(c) => c.as_str(),
            None => return true,
        };
        let span_start = match w.span {
            Some(s) => s.start,
            None => return true,
        };
        // Find the enclosing function
        for func in &module.functions {
            if span_start >= func.span.start && span_start < func.span.end {
                if let Some(codes) = suppressed.get(&func.span.start) {
                    return !codes.contains(code);
                }
                break;
            }
        }
        true
    });
}

/// Scan a text region for `@nera:nowarn(code)` patterns.
fn extract_nowarn_codes(text: &str) -> HashSet<String> {
    let mut codes = HashSet::new();
    let marker = "@nera:nowarn(";

    for line in text.lines() {
        let trimmed = line.trim();
        // Must be a comment line
        if !trimmed.starts_with('%') {
            continue;
        }
        // Search for the marker anywhere in the line
        if let Some(pos) = trimmed.find(marker) {
            let after = &trimmed[pos + marker.len()..];
            if let Some(end) = after.find(')') {
                let code = after[..end].trim();
                if !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    codes.insert(code.to_string());
                }
            }
        }
    }

    codes
}
