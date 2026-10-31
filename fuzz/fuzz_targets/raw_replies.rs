//! Backend bytes as a server or pooler might send them to a `send` client: the size
//! check, message parsing and the reply lines must not panic.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    poolproof::runner::fuzzing::replies(data);
});
