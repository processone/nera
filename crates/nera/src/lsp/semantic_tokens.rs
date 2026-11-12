use nera_syntax::semantic_tokens as sem;
use nera_syntax::source_map::SourceMap;

/// Convert [`sem::SemanticToken`]s into LSP delta-encoded [`SemanticToken`]s.
///
/// The input must be sorted by `span.start`.  Each LSP token is encoded
/// relative to the previous one: (delta_line, delta_start, length,
/// token_type, token_modifiers).
pub(crate) fn encode_semantic_tokens(
    tokens: &[sem::SemanticToken],
    sm: &SourceMap,
) -> Vec<tower_lsp::lsp_types::SemanticToken> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut prev_line: u32 = 0;
    let mut prev_start: u32 = 0;

    for tok in tokens {
        let start_pos = sm.offset_to_position(tok.span.start);
        let end_pos = sm.offset_to_position(tok.span.end);

        // SourceMap uses 1-based; LSP uses 0-based.
        let line = (start_pos.line - 1) as u32;
        let start_char = (start_pos.col - 1) as u32;
        let length = if start_pos.line == end_pos.line {
            (end_pos.col - start_pos.col) as u32
        } else {
            // Multi-line tokens: use length from start to end of first line.
            // This shouldn't normally happen for our token types.
            (tok.span.end - tok.span.start) as u32
        };

        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            start_char - prev_start
        } else {
            start_char
        };

        result.push(tower_lsp::lsp_types::SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: tok.token_type as u32,
            token_modifiers_bitset: tok.modifiers.0,
        });

        prev_line = line;
        prev_start = start_char;
    }

    result
}
