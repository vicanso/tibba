bump:
	@test -n "$(v)" || (echo "用法: make bump v=0.2.0" && exit 1)
	./bump_version.sh $(v)

publish:
	./scripts/publish.sh $(p)

scaffold:
	@test -n "$(name)" || (echo "用法: make scaffold name=my-app [output=~/github]" && exit 1)
	cargo run -p tibba-scaffold -- $(name) $(output)

lint:
	cargo clippy --all-targets --all -- --deny=warnings

fmt:
	cargo fmt

dev:
	bacon run

mermaid:
	cargo run --bin generate-mermaid

release:
	cargo build --release 