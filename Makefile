lint:
	cargo clippy --all-targets --all -- --deny=warnings

fmt:
	cargo fmt

dev:
	bacon run -- -p tibba-api


release:
	cargo build --release -p tibba-api 