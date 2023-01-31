lint:
	cargo clippy

fmt:
	cargo fmt

dev:
	cargo run

release:
	date > assets/build_date
	git rev-parse --short HEAD > assets/commit
	cargo build --release