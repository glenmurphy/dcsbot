#!/bin/sh
# sudo apt-get install musl-tools gcc make perl linux-headers-generic
# rustup target add x86_64-unknown-linux-musl
RUSTFLAGS='-C target-feature=+crt-static'
attr 
cargo build --target x86_64-unknown-linux-musl --release
cp ./target/x86_64-unknown-linux-musl/release/dcsbot ../dcsbot_deploy/
