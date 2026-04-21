pub mod types;
pub mod parser;
pub mod db;
pub mod refresh;
pub mod query;
pub mod completion;
pub mod uasset;
pub mod server;
pub mod modify;
pub mod vcs;

// Backward compatibility: existing code using `scanner::` continues to work.
// Future language parsers will live alongside cpp: parser::verse, parser::blueprint, etc.
pub mod scanner {
    pub use super::parser::cpp::*;
}
