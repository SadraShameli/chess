.DEFAULT_GOAL := verify
.NOTPARALLEL:

ARGS ?=
NATIVE_PROFILE ?= dev

.PHONY: verify format lint tests build profile-build run native-test-build native-tests legacy-native-test-build legacy-native-tests

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
	cargo clippy --locked --test native --all-features -- -D warnings

tests:
	cargo test --locked --all-features
	bun run --cwd ui test

build:
	bun run --cwd ui build
	cargo build --locked --release --all-targets --features tauri/custom-protocol
	cargo build --locked --release --test native --all-features

profile-build:
	bun run --cwd ui build
	cargo build --locked --profile profiling --all-targets --features tauri/custom-protocol
	cargo build --locked --profile profiling --test native --all-features

run:
	bun run --cwd ui tauri dev$(if $(strip $(ARGS)), -- -- $(ARGS))

native-test-build:
	cargo test --locked --profile $(NATIVE_PROFILE) --test native --all-features --no-run

native-tests: native-test-build
	cargo test --locked --profile $(NATIVE_PROFILE) --test native --all-features

legacy-native-test-build:
	bun run --cwd ui tauri build --no-bundle --features native-test --config tauri.test.conf.json -- --locked

legacy-native-tests: legacy-native-test-build
	bun run --cwd ui test:native
