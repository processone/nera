use std::path::Path;

use tower_lsp::lsp_types::{self, Position, Url};

use nera_syntax::source_map::SourceMap;
use nera_syntax::span::Span;

/// Build a mapping from original-source byte ranges to combined-source byte ranges.
///
/// When the preprocessor inlines `-include(...)` directives, the combined source
/// differs from the original file the editor shows. This mapping lets us convert
/// an offset in the original source (derived from an LSP position) to the
/// corresponding offset in the combined source (where AST spans live).
///
/// Returns a list of (original_start, original_end, combined_start) tuples for
/// each main-file segment. An original-source offset `o` in [orig_start, orig_end)
/// maps to combined offset `combined_start + (o - orig_start)`.
pub(crate) fn build_offset_map(
    original: &str,
    combined: &str,
    segments: &[nera_syntax::preproc::FileSegment],
    main_filename: &str,
) -> Vec<(usize, usize, usize)> {
    if original == combined {
        // No preprocessing changes — identity mapping
        return vec![(0, original.len(), 0)];
    }

    // Collect main-file segments from the combined source
    let main_segments: Vec<&nera_syntax::preproc::FileSegment> = segments.iter()
        .filter(|s| s.filename == main_filename)
        .collect();

    if main_segments.is_empty() {
        // Fallback: treat as identity
        return vec![(0, original.len(), 0)];
    }

    // The main-file segments in the combined source appear in order.
    // The original source is the concatenation of these segments' content
    // (with include directives replaced by their content in combined).
    // We need to figure out where each main-file segment starts in the original.
    //
    // Strategy: the combined source for main-file segments should match
    // substrings of the original source. We track our position in the original
    // and match each segment's content.
    let mut result = Vec::new();
    let mut orig_pos = 0;

    for seg in &main_segments {
        let seg_content = &combined[seg.start..seg.end.min(combined.len())];
        // Find this content in the original source starting from orig_pos
        if let Some(found) = original[orig_pos..].find(seg_content) {
            let orig_start = orig_pos + found;
            let orig_end = orig_start + seg_content.len();
            result.push((orig_start, orig_end, seg.start));
            orig_pos = orig_end;
        } else {
            // Content doesn't match exactly (macro expansion etc.) —
            // fall back to a heuristic: assume it starts at current position
            let seg_len = seg.end - seg.start;
            let orig_end = (orig_pos + seg_len).min(original.len());
            result.push((orig_pos, orig_end, seg.start));
            orig_pos = orig_end;
        }
    }

    if result.is_empty() {
        vec![(0, original.len(), 0)]
    } else {
        result
    }
}

/// Map an offset in the original source to the corresponding offset in the combined source.
pub(crate) fn map_original_to_combined(orig_offset: usize, offset_map: &[(usize, usize, usize)]) -> usize {
    for &(orig_start, orig_end, combined_start) in offset_map {
        if orig_offset >= orig_start && orig_offset < orig_end {
            return combined_start + (orig_offset - orig_start);
        }
    }
    // Past the last segment — return the end of the last combined range
    if let Some(&(orig_start, _, combined_start)) = offset_map.last() {
        combined_start + (orig_offset.saturating_sub(orig_start))
    } else {
        orig_offset
    }
}

/// Map a combined-source span to an LSP range in the original source.
/// Uses the offset map to reverse-map combined offsets back to original offsets,
/// then converts to LSP positions via the original source map.
pub(crate) fn span_to_original_lsp_range(
    span: Span,
    combined_sm: &SourceMap,
    original_sm: &SourceMap,
    offset_map: &[(usize, usize, usize)],
) -> lsp_types::Range {
    // Try to reverse-map combined offsets to original offsets
    let orig_start = map_combined_to_original(span.start, offset_map);
    let orig_end = map_combined_to_original(span.end, offset_map);
    if let (Some(os), Some(oe)) = (orig_start, orig_end) {
        let start = original_sm.offset_to_position(os);
        let end = original_sm.offset_to_position(oe);
        lsp_types::Range {
            start: Position {
                line: (start.line - 1) as u32,
                character: (start.col - 1) as u32,
            },
            end: Position {
                line: (end.line - 1) as u32,
                character: (end.col - 1) as u32,
            },
        }
    } else {
        // Fall back to combined source map (e.g., span is in an included file)
        span_to_lsp_range(span, combined_sm)
    }
}

/// Reverse-map a combined-source offset to the original-source offset.
/// Returns None if the offset is inside an included segment (not from the main file).
pub(crate) fn map_combined_to_original(combined_offset: usize, offset_map: &[(usize, usize, usize)]) -> Option<usize> {
    for &(orig_start, orig_end, combined_start) in offset_map {
        let combined_end = combined_start + (orig_end - orig_start);
        if combined_offset >= combined_start && combined_offset < combined_end {
            return Some(orig_start + (combined_offset - combined_start));
        }
    }
    None
}

/// Resolve a combined-source span to an LSP Location, correctly identifying
/// which file the span belongs to (main file or an included header).
///
/// Returns `(uri, range)` where `uri` points to the correct file.
pub(crate) fn resolve_span_to_location(
    span: Span,
    current_uri: &Url,
    file_segments: &[nera_syntax::preproc::FileSegment],
    combined_sm: &SourceMap,
    original_sm: &SourceMap,
    offset_map: &[(usize, usize, usize)],
    project_root: Option<&Path>,
) -> Option<(Url, lsp_types::Range)> {
    // Check if the span falls inside an included file segment
    // (a segment whose filename differs from the main file).
    if !file_segments.is_empty() {
        // The first segment is typically the main file (or the start of it).
        let main_filename = &file_segments[0].filename;

        for seg in file_segments {
            if span.start >= seg.start && span.start < seg.end && seg.filename != *main_filename {
                // Span is inside an included file — resolve to that file's URI and position
                let rel_start = span.start - seg.start;
                let rel_end = (span.end - seg.start).min(seg.end - seg.start);

                // Build a SourceMap for the included segment to convert offsets to lines
                let seg_source = combined_sm.source();
                let seg_slice = &seg_source[seg.start..seg.end.min(seg_source.len())];
                let seg_sm = SourceMap::new(seg_slice);

                let start_pos = seg_sm.offset_to_position(rel_start);
                let end_pos = seg_sm.offset_to_position(rel_end);
                let range = lsp_types::Range {
                    start: Position {
                        line: (start_pos.line - 1) as u32,
                        character: (start_pos.col - 1) as u32,
                    },
                    end: Position {
                        line: (end_pos.line - 1) as u32,
                        character: (end_pos.col - 1) as u32,
                    },
                };

                // Resolve the include file path to a URI
                let inc_path = Path::new(&seg.filename);
                let abs_path = if inc_path.is_absolute() {
                    inc_path.to_path_buf()
                } else if let Some(root) = project_root {
                    root.join(inc_path)
                } else {
                    inc_path.to_path_buf()
                };

                if let Ok(inc_uri) = Url::from_file_path(&abs_path) {
                    return Some((inc_uri, range));
                }
            }
        }
    }

    // Span is in the main file — map back to original source positions
    let range = span_to_original_lsp_range(span, combined_sm, original_sm, offset_map);
    Some((current_uri.clone(), range))
}

/// Convert a span to an LSP Range using a SourceMap.
pub(crate) fn span_to_lsp_range(span: Span, sm: &SourceMap) -> lsp_types::Range {
    let start = sm.offset_to_position(span.start);
    let end = sm.offset_to_position(span.end);
    lsp_types::Range {
        start: Position {
            line: (start.line - 1) as u32,
            character: (start.col - 1) as u32,
        },
        end: Position {
            line: (end.line - 1) as u32,
            character: (end.col - 1) as u32,
        },
    }
}
