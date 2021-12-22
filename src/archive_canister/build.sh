#!/usr/bin/env bash

cargo build --target wasm32-unknown-unknown --release --package archive_canister && \
 ic-cdk-optimizer ./target/wasm32-unknown-unknown/release/archive_canister.wasm -o ./target/wasm32-unknown-unknown/release/archive_canister.wasm

#  cargo build --target wasm32-unknown-unknown --package archive_canister --release