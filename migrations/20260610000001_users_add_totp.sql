-- 给 users 表添加 TOTP 两步验证（2FA）相关字段。
-- 三态模型：
--   totp_secret IS NULL                          → 未注册 2FA
--   totp_secret 有值 AND totp_enabled_at IS NULL  → 已生成密钥但未激活（待确认）
--   totp_secret 有值 AND totp_enabled_at 有值     → 2FA 已启用，登录强制校验
ALTER TABLE users
  ADD COLUMN IF NOT EXISTS totp_secret TEXT DEFAULT NULL,
  ADD COLUMN IF NOT EXISTS totp_enabled_at TIMESTAMP DEFAULT NULL,
  ADD COLUMN IF NOT EXISTS totp_recovery_codes JSONB DEFAULT NULL;

COMMENT ON COLUMN users.totp_secret IS 'TOTP 密钥，AES-256-GCM 加密后 base64；NULL 表示未注册 2FA';
COMMENT ON COLUMN users.totp_enabled_at IS '2FA 激活时间；NULL 表示未启用（仅生成密钥未确认）';
COMMENT ON COLUMN users.totp_recovery_codes IS '一次性恢复码的 SHA-256(base64) 数组，消费一个即从数组移除';
