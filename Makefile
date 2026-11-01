.PHONY: check fmt test build

check:
	cargo fmt --all -- --check
	cargo clippy --all-targets --locked -- -D warnings
	cargo test --locked

fmt:
	cargo fmt --all

test:
	cargo test --locked

build:
	cargo build --release --locked
