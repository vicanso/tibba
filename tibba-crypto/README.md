# tibba-crypto

**密码学**

> **分层**：核心（Core）— REST 脚手架底座，优先发布

提供密码哈希（Argon2id + Pepper）、密钥轮换 HMAC 签名（KeyGrip）与字段级对称加密（AES-256-GCM）。

## 核心能力

- **密码哈希（`password`）**：`PasswordPolicy` 支持自定义 Argon2id 参数、最大口令长度限制（`DEFAULT_MAX_SECRET_LEN` = 1024 字节，防止超长口令触发 CPU DoS）与可选 `.with_pepper(&'static [u8])`；可通过 `init_global_password_policy` / `global_password_policy` 全局共享。`verify_password` 返回 `PasswordCheck`（`Matched` / `MatchedNeedsRehash` / `Mismatched`），支持无感升级历史明文 sha256 或弱参数哈希
- **HMAC 签名轮换（`key_grip`）**：`KeyGrip` 使用多密钥列表进行 HMAC-SHA256 签名与验签，`Verification::needs_resign()` 指示历史旧密钥签发的数据，支持 `update_keys` 平滑轮换
- **字段对称加密（`cipher`）**：`SecretCipher` 基于 AES-256-GCM（`from_app_secret` / `from_key`），支持 `with_fallback_*` 密钥平滑迁移、`encrypt_with_aad` / `decrypt_with_aad` 绑定额外认证数据，以及 `decrypt_and_check_rotation*`

## 依赖

依赖：tibba-error

## 在工作区中的位置

- 版本：与 workspace 统一（`version.workspace = true`，当前见根 `Cargo.toml` `[workspace.package]`）
- 发布：见 `scripts/publish.sh`（`core` / `ext` 分组）

## 相关文档

- [crate 分层说明](../docs/crates.md)
- [模块依赖图](../docs/modules.md)
