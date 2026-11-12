//! Source map for converting byte offsets to line:column positions.
//!
//! The `SourceMap` stores a pre-computed line index (the byte offset of
//! each line start) so that `offset_to_position()` is a cheap binary search.

use std::ops::Range;

use codespan_reporting::files;

use crate::span::Span;

/// A 1-based line and column position in source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number (in bytes, not characters).
    pub col: usize,
}

impl std::fmt::Display for LineCol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Pre-computed line index for a source file.
///
/// Build once from source text, then use `offset_to_position()` to
/// convert any byte offset to a `LineCol`.
#[derive(Debug, Clone)]
pub struct SourceMap {
    /// The original source text.
    source: String,
    /// Byte offsets of the start of each line (line_starts[0] == 0).
    line_starts: Vec<usize>,
    /// Optional filename for error messages.
    filename: Option<String>,
}

impl SourceMap {
    /// Build a source map from the given source text.
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            source: source.to_string(),
            line_starts,
            filename: None,
        }
    }

    /// Build a source map with an associated filename.
    pub fn with_filename(source: &str, filename: impl Into<String>) -> Self {
        let mut sm = Self::new(source);
        sm.filename = Some(filename.into());
        sm
    }

    /// Return the filename, if set.
    pub fn filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    /// Return a reference to the source text.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Convert a 0-based line and column (as used by LSP) to a byte offset.
    ///
    /// `line_starts` is 0-indexed (entry 0 = line 1's start offset), so a
    /// 0-based LSP line indexes directly into it.  Clamps to source length.
    pub fn position_to_offset(&self, line: u32, col: u32) -> usize {
        let line_idx = line as usize;
        if line_idx >= self.line_starts.len() {
            return self.source.len();
        }
        let line_start = self.line_starts[line_idx];
        (line_start + col as usize).min(self.source.len())
    }

    /// Convert a byte offset to a 1-based `LineCol`.
    ///
    /// If the offset is past the end of the source, it clamps to the last
    /// valid position.
    pub fn offset_to_position(&self, offset: usize) -> LineCol {
        let offset = offset.min(self.source.len());
        // Binary search for the line that contains this offset.
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(insert) => insert.saturating_sub(1),
        };
        let col = offset - self.line_starts[line_idx];
        LineCol {
            line: line_idx + 1, // 1-based
            col: col + 1,       // 1-based
        }
    }

    /// Convert a `Span` to a pair of `LineCol` positions.
    pub fn span_to_positions(&self, span: Span) -> (LineCol, LineCol) {
        (
            self.offset_to_position(span.start),
            self.offset_to_position(span.end),
        )
    }

    /// Return the source text for the given 1-based line number.
    ///
    /// Returns `None` if the line number is out of range.
    pub fn line_text(&self, line: usize) -> Option<&str> {
        if line == 0 || line > self.line_starts.len() {
            return None;
        }
        let start = self.line_starts[line - 1];
        let end = if line < self.line_starts.len() {
            self.line_starts[line]
        } else {
            self.source.len()
        };
        // Trim trailing newline
        let text = &self.source[start..end];
        Some(text.trim_end_matches('\n').trim_end_matches('\r'))
    }

    /// Return the number of lines in the source.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Format a location string like `file:line:col` or just `line:col`.
    pub fn format_location(&self, offset: usize) -> String {
        let pos = self.offset_to_position(offset);
        match &self.filename {
            Some(f) => format!("{}:{}:{}", f, pos.line, pos.col),
            None => format!("{}:{}", pos.line, pos.col),
        }
    }

    /// Format a span location string like `file:line:col`.
    pub fn format_span_location(&self, span: Span) -> String {
        self.format_location(span.start)
    }

    /// Render a source snippet with a caret pointing at the error span.
    ///
    /// Example output:
    /// ```text
    ///   4 |     io:format("Hello"))
    ///                              ^
    /// ```
    pub fn render_snippet(&self, span: Span) -> String {
        let start_pos = self.offset_to_position(span.start);
        let end_pos = self.offset_to_position(span.end);

        let mut output = String::new();

        // Show the line containing the error start
        if let Some(line_text) = self.line_text(start_pos.line) {
            let line_num = format!("{}", start_pos.line);
            let gutter_width = line_num.len();

            // The source line
            output.push_str(&format!(
                " {} | {}\n",
                line_num, line_text
            ));

            // The caret line
            let padding = " ".repeat(gutter_width + 3); // " N | " prefix
            let col_padding = " ".repeat(start_pos.col - 1);

            let highlight_len = if start_pos.line == end_pos.line {
                (end_pos.col - start_pos.col).max(1)
            } else {
                // Span crosses lines — underline to end of first line
                (line_text.len() + 1 - start_pos.col).max(1)
            };
            let carets = "^".repeat(highlight_len);

            output.push_str(&format!("{}{}{}\n", padding, col_padding, carets));
        }

        output
    }
}

/// Implement codespan-reporting's `Files` trait so that `SourceMap` can be
/// passed directly to `codespan_reporting::term::emit()`.
///
/// Since each `SourceMap` represents a single file, we use `()` as the file ID.
impl<'a> files::Files<'a> for SourceMap {
    type FileId = ();
    type Name = &'a str;
    type Source = &'a str;

    fn name(&'a self, _id: ()) -> Result<Self::Name, files::Error> {
        Ok(self.filename.as_deref().unwrap_or("<unknown>"))
    }

    fn source(&'a self, _id: ()) -> Result<Self::Source, files::Error> {
        Ok(&self.source)
    }

    fn line_index(&self, _id: (), byte_index: usize) -> Result<usize, files::Error> {
        match self.line_starts.binary_search(&byte_index) {
            Ok(line) => Ok(line),
            Err(next_line) => Ok(next_line.saturating_sub(1)),
        }
    }

    fn line_range(&self, _id: (), line_index: usize) -> Result<Range<usize>, files::Error> {
        let num_lines = self.line_starts.len();
        if line_index >= num_lines {
            return Err(files::Error::LineTooLarge {
                given: line_index,
                max: num_lines.saturating_sub(1),
            });
        }
        let start = self.line_starts[line_index];
        let end = if line_index + 1 < num_lines {
            self.line_starts[line_index + 1]
        } else {
            self.source.len()
        };
        Ok(start..end)
    }
}

/// A segment in a `MultiSourceMap`: a region of the combined source that
/// belongs to a specific file.
#[derive(Debug, Clone)]
struct MultiSegment {
    /// Byte offset in combined_source where this segment starts.
    start_offset: usize,
    /// Byte offset in combined_source where this segment ends (exclusive).
    end_offset: usize,
    /// The filename for this segment.
    filename: String,
    /// Pre-computed line starts (byte offsets relative to `start_offset`).
    line_starts: Vec<usize>,
}

/// Multi-file source map for error reporting across included files.
///
/// Tracks which byte ranges in the combined source buffer belong to which file,
/// enabling `codespan-reporting` to display the correct filename and line number
/// for errors in `.hrl` header files.
#[derive(Debug, Clone)]
pub struct MultiSourceMap {
    /// The combined source text (main file + all included files).
    combined_source: String,
    /// File segments, sorted by `start_offset`.
    segments: Vec<MultiSegment>,
}

impl MultiSourceMap {
    /// Build a `MultiSourceMap` from the combined source and file segments
    /// produced by the preprocessor.
    pub fn new(
        combined_source: String,
        segments: Vec<crate::preproc::FileSegment>,
    ) -> Self {
        let multi_segments = segments
            .into_iter()
            .map(|seg| {
                // Compute line starts relative to segment start.
                let slice = &combined_source[seg.start..seg.end.min(combined_source.len())];
                let mut line_starts = vec![0usize];
                for (i, b) in slice.bytes().enumerate() {
                    if b == b'\n' {
                        line_starts.push(i + 1);
                    }
                }
                MultiSegment {
                    start_offset: seg.start,
                    end_offset: seg.end,
                    filename: seg.filename,
                    line_starts,
                }
            })
            .collect();

        Self {
            combined_source,
            segments: multi_segments,
        }
    }

    /// Build a single-file `SourceMap` from the combined source.
    ///
    /// Used as a fallback for APIs that require a `SourceMap` (e.g. IR lowering),
    /// where only line numbers matter and per-file attribution is not needed.
    pub fn to_source_map(&self, filename: &str) -> SourceMap {
        SourceMap::with_filename(&self.combined_source, filename)
    }

    /// Return the start byte offset of the given segment in the combined source.
    pub fn segment_start(&self, id: usize) -> usize {
        self.segments.get(id).map(|s| s.start_offset).unwrap_or(0)
    }

    /// Return the filename for the given segment.
    pub fn segment_filename(&self, id: usize) -> Option<&str> {
        self.segments.get(id).map(|s| s.filename.as_str())
    }

    /// Return the number of file segments.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Return the combined source text.
    pub fn combined_source(&self) -> &str {
        &self.combined_source
    }

    /// Find the segment index for a given byte offset in the combined source.
    pub fn file_id_for_offset(&self, offset: usize) -> usize {
        // Search backwards so that later (more specific) segments for included
        // files take priority over the main file segment that covers the prefix.
        for (i, seg) in self.segments.iter().enumerate().rev() {
            if offset >= seg.start_offset && offset < seg.end_offset {
                return i;
            }
        }
        // Fallback: first segment (the main file).
        0
    }
}

/// Implement `codespan-reporting`'s `Files` trait with `FileId = usize`
/// (the segment index).
impl<'a> files::Files<'a> for MultiSourceMap {
    type FileId = usize;
    type Name = &'a str;
    type Source = &'a str;

    fn name(&'a self, id: usize) -> Result<Self::Name, files::Error> {
        self.segments
            .get(id)
            .map(|seg| seg.filename.as_str())
            .ok_or(files::Error::FileMissing)
    }

    fn source(&'a self, id: usize) -> Result<Self::Source, files::Error> {
        self.segments
            .get(id)
            .map(|seg| &self.combined_source[seg.start_offset..seg.end_offset.min(self.combined_source.len())])
            .ok_or(files::Error::FileMissing)
    }

    fn line_index(&self, id: usize, byte_index: usize) -> Result<usize, files::Error> {
        let seg = self.segments.get(id).ok_or(files::Error::FileMissing)?;
        // byte_index is relative to the segment source (codespan-reporting
        // passes offsets relative to the source returned by `source()`).
        let clamped = byte_index.min(seg.end_offset.saturating_sub(seg.start_offset));
        match seg.line_starts.binary_search(&clamped) {
            Ok(line) => Ok(line),
            Err(next_line) => Ok(next_line.saturating_sub(1)),
        }
    }

    fn line_range(&self, id: usize, line_index: usize) -> Result<Range<usize>, files::Error> {
        let seg = self.segments.get(id).ok_or(files::Error::FileMissing)?;
        let num_lines = seg.line_starts.len();
        if line_index >= num_lines {
            return Err(files::Error::LineTooLarge {
                given: line_index,
                max: num_lines.saturating_sub(1),
            });
        }
        let start = seg.line_starts[line_index];
        let seg_len = seg.end_offset.saturating_sub(seg.start_offset);
        let end = if line_index + 1 < num_lines {
            seg.line_starts[line_index + 1]
        } else {
            seg_len
        };
        Ok(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line() {
        let sm = SourceMap::new("hello world");
        assert_eq!(sm.offset_to_position(0), LineCol { line: 1, col: 1 });
        assert_eq!(sm.offset_to_position(5), LineCol { line: 1, col: 6 });
        assert_eq!(sm.offset_to_position(11), LineCol { line: 1, col: 12 });
    }

    #[test]
    fn multi_line() {
        let src = "line one\nline two\nline three\n";
        let sm = SourceMap::new(src);

        // Start of each line
        assert_eq!(sm.offset_to_position(0), LineCol { line: 1, col: 1 });
        assert_eq!(sm.offset_to_position(9), LineCol { line: 2, col: 1 });
        assert_eq!(sm.offset_to_position(18), LineCol { line: 3, col: 1 });

        // Middle of lines
        assert_eq!(sm.offset_to_position(5), LineCol { line: 1, col: 6 });
        assert_eq!(sm.offset_to_position(14), LineCol { line: 2, col: 6 });
    }

    #[test]
    fn line_text_retrieval() {
        let src = "first\nsecond\nthird";
        let sm = SourceMap::new(src);

        assert_eq!(sm.line_text(1), Some("first"));
        assert_eq!(sm.line_text(2), Some("second"));
        assert_eq!(sm.line_text(3), Some("third"));
        assert_eq!(sm.line_text(0), None);
        assert_eq!(sm.line_text(4), None);
    }

    #[test]
    fn with_filename() {
        let sm = SourceMap::with_filename("hello", "test.erl");
        assert_eq!(sm.format_location(0), "test.erl:1:1");
    }

    #[test]
    fn snippet_rendering() {
        let src = "-module(test).\nstart() ->\n    io:format(\"Hello\"))\n";
        let sm = SourceMap::with_filename(src, "test.erl");

        // Span pointing at the extra ')' at end of line 3
        let span = Span::new(46, 47); // the extra ')'
        let snippet = sm.render_snippet(span);

        // Should contain line number and caret
        assert!(snippet.contains("3 |"));
        assert!(snippet.contains("^"));
    }

    #[test]
    fn offset_past_end_clamps() {
        let sm = SourceMap::new("short");
        let pos = sm.offset_to_position(999);
        assert_eq!(pos, LineCol { line: 1, col: 6 });
    }

    #[test]
    fn empty_source() {
        let sm = SourceMap::new("");
        assert_eq!(sm.offset_to_position(0), LineCol { line: 1, col: 1 });
        assert_eq!(sm.line_count(), 1);
    }

    #[test]
    fn line_count() {
        assert_eq!(SourceMap::new("").line_count(), 1);
        assert_eq!(SourceMap::new("one").line_count(), 1);
        assert_eq!(SourceMap::new("one\ntwo").line_count(), 2);
        assert_eq!(SourceMap::new("one\ntwo\n").line_count(), 3);
    }

    #[test]
    fn position_to_offset_single_line() {
        let sm = SourceMap::new("hello world");
        // 0-based: line 0, col 0 → offset 0
        assert_eq!(sm.position_to_offset(0, 0), 0);
        // line 0, col 5 → offset 5
        assert_eq!(sm.position_to_offset(0, 5), 5);
    }

    #[test]
    fn position_to_offset_multi_line() {
        let src = "line one\nline two\nline three\n";
        let sm = SourceMap::new(src);
        // line 0, col 0 → 0
        assert_eq!(sm.position_to_offset(0, 0), 0);
        // line 1, col 0 → 9 (start of "line two")
        assert_eq!(sm.position_to_offset(1, 0), 9);
        // line 1, col 5 → 14
        assert_eq!(sm.position_to_offset(1, 5), 14);
        // line 2, col 0 → 18 (start of "line three")
        assert_eq!(sm.position_to_offset(2, 0), 18);
    }

    #[test]
    fn position_to_offset_clamps() {
        let sm = SourceMap::new("short");
        // Line past end clamps to source length
        assert_eq!(sm.position_to_offset(99, 0), 5);
        // Column past end clamps to source length
        assert_eq!(sm.position_to_offset(0, 99), 5);
    }

    #[test]
    fn position_to_offset_roundtrip() {
        let src = "abc\ndef\nghi";
        let sm = SourceMap::new(src);
        // offset 4 is 'd' → LineCol { line: 2, col: 1 } → 0-based (1, 0) → offset 4
        let pos = sm.offset_to_position(4);
        let offset = sm.position_to_offset((pos.line - 1) as u32, (pos.col - 1) as u32);
        assert_eq!(offset, 4);
    }

    #[test]
    fn span_to_positions() {
        let src = "abc\ndef\nghi";
        let sm = SourceMap::new(src);
        let (start, end) = sm.span_to_positions(Span::new(4, 7));
        assert_eq!(start, LineCol { line: 2, col: 1 });
        assert_eq!(end, LineCol { line: 2, col: 4 });
    }

    #[test]
    fn multi_source_map_segment_filename() {
        let combined = "line one\nline two\nincluded line\n";
        let segments = vec![
            crate::preproc::FileSegment {
                start: 0,
                end: 18,
                filename: "main.erl".to_string(),
            },
            crate::preproc::FileSegment {
                start: 18,
                end: 32,
                filename: "header.hrl".to_string(),
            },
        ];
        let msm = MultiSourceMap::new(combined.to_string(), segments);

        assert_eq!(msm.segment_count(), 2);
        assert_eq!(msm.segment_filename(0), Some("main.erl"));
        assert_eq!(msm.segment_filename(1), Some("header.hrl"));
        assert_eq!(msm.segment_filename(2), None);
    }

    #[test]
    fn multi_source_map_combined_source() {
        let combined = "hello world";
        let segments = vec![crate::preproc::FileSegment {
            start: 0,
            end: 11,
            filename: "test.erl".to_string(),
        }];
        let msm = MultiSourceMap::new(combined.to_string(), segments);
        assert_eq!(msm.combined_source(), "hello world");
    }
}
