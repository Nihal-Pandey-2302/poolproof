# Fuzz targets

Two cargo-fuzz targets for the code that reads input poolproof doesn't control. They are
not part of the poolproof crate or CI.

- `raw_replies`: arbitrary bytes as a server or pooler might send them to a `send`
  client, through the message size check, message parsing and the reply lines.
- `send_messages`: arbitrary text as `send` lines in a scenario file, parsed and encoded.

With [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) and a nightly toolchain, from
this directory:

```sh
ASAN_OPTIONS=detect_odr_violation=0 cargo +nightly fuzz run raw_replies -- -max_total_time=600
```

`detect_odr_violation=0` is needed because pg_query's vendored C code (upb) defines a
global twice, which AddressSanitizer reports at startup. It has nothing to do with the
code under test.

Without cargo-fuzz, the same build by hand:

```sh
RUSTFLAGS="-Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=4 \
  -Cllvm-args=-sanitizer-coverage-inline-8bit-counters -Cllvm-args=-sanitizer-coverage-pc-table \
  -Cllvm-args=-sanitizer-coverage-trace-compares --cfg fuzzing -Cdebug-assertions \
  -Coverflow-checks -Zsanitizer=address" \
  cargo +nightly build --release --target x86_64-unknown-linux-gnu --bins
mkdir -p corpus/raw_replies
ASAN_OPTIONS=detect_odr_violation=0 \
  ./target/x86_64-unknown-linux-gnu/release/raw_replies corpus/raw_replies -max_total_time=600
```

`corpus/`, `artifacts/` and `target/` are not committed. Good seeds for `send_messages`
are the `send` lines of the bundled scenarios.
