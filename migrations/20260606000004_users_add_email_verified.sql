-- 给 users 表添加邮箱验证通过时间字段。
-- NULL 表示该用户邮箱尚未验证（兼容旧数据：所有历史用户默认未验证）。
-- 由邮箱验证流程（POST /email/verify/confirm）在验证成功时写入 NOW()。
ALTER TABLE users
  ADD COLUMN IF NOT EXISTS email_verified_at TIMESTAMP DEFAULT NULL;

COMMENT ON COLUMN users.email_verified_at IS '邮箱验证通过时间，NULL 表示未验证';
