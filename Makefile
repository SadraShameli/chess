.DEFAULT_GOAL := verify
.NOTPARALLEL:

ARGS ?=

.PHONY: verify format lint tests build profile-build run native-test-build native-tests

verify: format lint tests build

format:
	cargo fmt --all
	bun run --cwd ui format
	bun run --cwd ui lint:styles --fix

lint:
	cargo fmt --all --check
	bun run --cwd ui format:check
	bun run --cwd ui typecheck
	bun run --cwd ui lint
	cargo clippy --locked --all-targets --all-features -- -D warnings

tests:
	cargo test --locked --all-features
	bun run --cwd ui test

build:
	bun run --cwd ui build
	cargo build --locked --release --all-targets --features tauri/custom-protocol

profile-build:
	bun run --cwd ui build
	cargo build --locked --profile profiling --all-targets --features tauri/custom-protocol

run:
	bun run --cwd ui tauri dev --release$(if $(strip $(ARGS)), -- -- $(ARGS))

native-test-build:
	bun run --cwd ui tauri build --no-bundle --features native-test --config tauri.test.conf.json -- --locked

native-tests: native-test-build
	bun run --cwd ui test:native
