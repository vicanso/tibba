CREATE TABLE `permissions` (
  `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT COMMENT '主键ID',
  `code` VARCHAR(100) NOT NULL COMMENT '权限码，形如 "resource:action"；"*" 为通配；"resource:*" 为前缀通配',
  `description` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '给运维/管理面板的描述文案',
  `created` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `modified` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `deleted_at` DATETIME DEFAULT NULL COMMENT '软删除时间',
  PRIMARY KEY (`id`) COMMENT '主键',
  UNIQUE KEY `uk_permissions_code` (`code`, `deleted_at`) COMMENT '权限码唯一索引（仅对未删除记录生效）',
  KEY `idx_deleted_at` (`deleted_at`) COMMENT '软删除索引'
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT="权限点表，登记 RBAC 中所有可被授予的原子权限码";
