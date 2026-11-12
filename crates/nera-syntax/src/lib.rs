pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod preproc;
pub mod span;
pub mod error;
pub mod source_map;
pub mod semantic_tokens;

// Re-export key types for convenience.
pub use preproc::FileSegment;
pub use source_map::MultiSourceMap;
