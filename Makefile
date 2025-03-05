lint:
	cargo clippy

fmt:
	cargo fmt

dev:
	cargo watch -w src -x 'run'

udeps:
	cargo +nightly udeps

install:
	cargo install sea-orm-cli

entity:
	sea-orm-cli generate entity --with-serde=both -u mysql://vicanso:A123456@127.0.0.1:3306/tibba -o src/entities

release:
	date > assets/build_date
	git rev-parse --short HEAD > assets/commit
	cargo build --release