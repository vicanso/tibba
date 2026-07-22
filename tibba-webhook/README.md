# tibba-webhook

**出站 Webhook**

> **分层**：扩展（Extension）— 可选能力，依赖核心后再发布

HMAC 签名的出站 webhook 投递，复用 job 队列做重试与死信。

## 依赖

依赖：tibba-crypto, tibba-error, tibba-job, tibba-request, tibba-util

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
