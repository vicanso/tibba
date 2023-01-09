lint:
	cargo clippy

fmt:
	cargo fmt

dev:
	cargo run

release:
	date > assets/build_date
	cargo build --release