# admin 前端构建产物目录

本目录供：

1. **Vite 构建输出**（`npm run build` → `index.html`、`assets/*`）
2. **Rust `rust-embed`**（`src/admin_web.rs` 的 `#[folder = "admin/dist/"]`）

`rust-embed` 在**编译期**要求该目录存在。因此仓库中保留本 `README.md` 作为占位，确保 CI / 未先构建前端时 `cargo build` 仍可通过。

## 本地开发

```bash
cd admin
npm ci
npm run build   # 生成 index.html 与 assets/
```

生产镜像在 Docker 多阶段构建里会先执行 admin build，再编译 Rust，产物覆盖本目录内容。

## 注意

- 除本 README 外的构建产物默认不入库（见根 `.gitignore`）
- 仅有 README、无 `index.html` 时，SPA 回退会返回 404，属预期
