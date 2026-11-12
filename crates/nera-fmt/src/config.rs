/// Formatting configuration.
#[derive(Debug, Clone)]
pub struct FormatConfig {
    /// Maximum line width before breaking.
    pub max_width: usize,
    /// Number of spaces per indentation level.
    pub indent_size: usize,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            max_width: 100,
            indent_size: 4,
        }
    }
}
