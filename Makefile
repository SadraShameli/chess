.DEFAULT_GOAL := verify
.NOTPARALLEL:

ARGS ?=

.PHONY: verify format lint tests build profile-build run

verify: format lint tests build

format:
	cargo fmt --all

lint:
	cargo fmt --all --check
	cargo clippy --locked --all-targets --all-features -- -D warnings

tests:
	cargo test --locked --all-features

build:
	cargo build --locked --release --all-targets --all-features

profile-build:
	cargo build --locked --profile profiling --all-targets --all-features

run:
	cargo run --locked --release$(if $(strip $(ARGS)), -- $(ARGS))
