lint:
	cargo clippy --all-targets --all -- --deny=warnings

fmt:
	cargo fmt

dev:
	bacon run -- -p tibba-api


release:
	date > assets/build_date
	git rev-parse --short HEAD > assets/commit
	cargo build --release