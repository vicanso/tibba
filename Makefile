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
	@echo "updated docs/modules.md"

release:
	cargo build --release

# 最小二进制（关掉 docker/detector/tenant 样板）
release-minimal:
	cargo build --release --no-default-features

# 导出 OpenAPI JSON（供 admin 生成 TS client）
openapi:
	cargo run --bin export-openapi -- admin/openapi.json

openapi-types: openapi
	cd admin && ./scripts/gen-api-types.sh openapi.json src/api/schema.d.ts 