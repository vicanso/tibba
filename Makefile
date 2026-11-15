bump:
	@test -n "$(v)" || (echo "用法: make bump v=0.2.0" && exit 1)
	./bump_version.sh $(v)

# 发布到 crates.io：make publish / make publish p=core / make publish p=standard / make publish p=tibba-error
publish:
	./scripts/publish.sh $(p)

# 派生新项目：复制本仓库并改名（详见 docs/scaffold.md）
# make init name=my-app dest=~/github [flags=--minimal]
init:
	@test -n "$(name)" || (echo "用法: make init name=my-app dest=~/github [flags=--minimal]" && exit 1)
	@test -n "$(dest)" || (echo "用法: make init name=my-app dest=~/github [flags=--minimal]" && exit 1)
	./scripts/init-project.sh $(name) $(dest) $(flags)

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

# 内嵌 admin/dist SPA 静态资源的发布二进制
release-embed:
	cargo build --release --features embed-admin

# 最小二进制（关闭 default features 中的 file / job）
release-minimal:
	cargo build --release --no-default-features

# 导出全量 OpenAPI JSON（供 admin 生成 TS client）
openapi:
	cargo run --features full --bin export-openapi -- admin/openapi.json

openapi-types: openapi
	cd admin && ./scripts/gen-api-types.sh openapi.json src/api/schema.d.ts 