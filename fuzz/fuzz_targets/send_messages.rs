//! `send` lines as written in scenario files: parsing and encoding must not panic.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    poolproof::runner::fuzzing::send_messages(text);
});
