//! poolproof as a library, so integration tests in `tests/` can use its modules.
//! The command-line tool is the product: nothing here is a stable API.

pub mod cli;
pub mod output;
pub mod report;
pub mod runner;
pub mod scan;
pub mod scenario;
pub mod targets;
