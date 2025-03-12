lint:
	# cargo clippy --all-targets --all -- --deny=warnings

fmt:
	cargo fmt

dev:
	cargo watch -w src -x 'run -p tibba-web'


release:
	date > assets/build_date
	git rev-parse --short HEAD > assets/commit
	cargo build --release