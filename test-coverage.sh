#!/usr/bin/env bash
cd "$(dirname "$0")"

export CARGO_INCREMENTAL=0 
export RUSTFLAGS='-Cinstrument-coverage' 
export LLVM_PROFILE_FILE='cargo-test-%p-%m.profraw'
cargo build
cargo test
grcov . --binary-path ./target/debug/deps/ -s . -t html --branch --ignore-not-existing --ignore '../*' --ignore "/*" \
  --excl-start '^[ ]*// cov ignore \{[ ]*$' --excl-stop '^[ ]*// \} cov ignore[ ]*$' \
  -o target/coverage/html
rm ./cargo-test-*
