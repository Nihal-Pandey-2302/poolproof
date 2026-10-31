//! poolproof as a library, so integration tests in `tests/` can use its modules.
//! The command-line tool is the product: nothing here is a stable API, so the modules
//! are hidden from the generated docs.

#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod doctor;
#[doc(hidden)]
pub mod output;
#[doc(hidden)]
pub mod report;
#[doc(hidden)]
pub mod runner;
#[doc(hidden)]
pub mod scan;
#[doc(hidden)]
pub mod scenario;
#[doc(hidden)]
pub mod targets;
