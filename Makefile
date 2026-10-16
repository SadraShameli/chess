.DEFAULT_GOAL := verify
.NOTPARALLEL:

ARGS ?=
NATIVE_PROFILE ?= dev

.PHONY: verify format lint tests build profile-build run native-test-build native-tests

verify: format lint tests build

format:
	cargo fmt --all

lint:
	cargo fmt --all --check
	cargo clippy --locked --bin chess-tools -- -D warnings
	cargo clippy --locked --all-targets --all-features -- -D warnings
	cargo clippy --locked --test native --all-features -- -D warnings

tests:
	cargo test --locked --all-features

build:
	cargo build --locked --release --all-targets --all-features
	cargo build --locked --release --test native --all-features

profile-build:
	cargo build --locked --profile profiling --all-targets --all-features
	cargo build --locked --profile profiling --test native --all-features

run:
	cargo run --locked$(if $(strip $(ARGS)), -- $(ARGS))

native-test-build:
	cargo test --locked --profile $(NATIVE_PROFILE) --test native --all-features --no-run

native-tests: native-test-build
	cargo test --locked --profile $(NATIVE_PROFILE) --test native --all-features
