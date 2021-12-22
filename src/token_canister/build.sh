#!/usr/bin/env bash

cargo build --target wasm32-unknown-unknown --release --package token_canister && \
 ic-cdk-optimizer ./target/wasm32-unknown-unknown/release/token_canister.wasm -o ./target/wasm32-unknown-unknown/release/token_canister.wasm

#  cargo build --target wasm32-unknown-unknown --package token_canister --release