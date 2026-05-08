lint:
	cargo clippy --all-targets --all -- --deny=warnings

fmt:
	cargo fmt

dev:
	bacon run

admin:
	cd admin && npm install && npm run build

release:
	cargo build --release
