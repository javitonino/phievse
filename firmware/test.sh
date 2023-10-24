#!/bin/sh

# Build std with standard features for testing. Default for microcontroller is to
# abort on panic which doesn't play well with asserts (which panic)
cargo test --target $(rustc -Vv | grep host | cut -d' ' -f2) -Z build-std-features= --lib "$@"
