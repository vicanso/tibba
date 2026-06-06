CREATE TABLE `user_oauth_links` (
  `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT COMMENT '主键ID',
  `user_id` BIGINT UNSIGNED NOT NULL COMMENT '本地 users.id',
  `provider` VARCHAR(32) NOT NULL COMMENT '第三方 provider 名，如 "github"',
  `provider_user_id` VARCHAR(64) NOT NULL COMMENT '第三方用户稳定 id（GitHub 数字 id / Google sub）',
  `provider_login` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '第三方 username 快照（展示用）',
  `provider_email` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '第三方 primary verified email 快照',
  `created` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `deleted_at` DATETIME DEFAULT NULL COMMENT '软删除时间（解绑后写入）',
  PRIMARY KEY (`id`) COMMENT '主键',
  UNIQUE KEY `uk_provider_uid` (`provider`, `provider_user_id`, `deleted_at`) COMMENT '同一 provider 用户不可绑多个本地账号',
  KEY `idx_user_id` (`user_id`, `deleted_at`) COMMENT '按本地用户反查已绑列表',
  KEY `idx_deleted_at` (`deleted_at`) COMMENT '软删除索引'
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT="用户与第三方身份提供商的关联表";
