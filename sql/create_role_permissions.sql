CREATE TABLE `role_permissions` (
  `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT COMMENT '主键ID',
  `role` VARCHAR(64) NOT NULL COMMENT '角色名，与 users.roles 中存储的字符串保持一致（如 "su"、"admin"）',
  `permission_code` VARCHAR(100) NOT NULL COMMENT '权限码，与 permissions.code 对应；不加外键以支持通配权限码',
  `created` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `deleted_at` DATETIME DEFAULT NULL COMMENT '软删除时间',
  PRIMARY KEY (`id`) COMMENT '主键',
  UNIQUE KEY `uk_role_permission` (`role`, `permission_code`, `deleted_at`) COMMENT '角色-权限映射唯一索引（仅对未删除记录生效）',
  KEY `idx_role` (`role`, `deleted_at`) COMMENT '按角色查询索引',
  KEY `idx_deleted_at` (`deleted_at`) COMMENT '软删除索引'
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT="角色到权限的多对多映射表";

-- 种子数据：登记通配权限码并授予超级管理员角色
INSERT IGNORE INTO `permissions` (`code`, `description`)
VALUES ('*', 'Wildcard — grants every action');

INSERT IGNORE INTO `role_permissions` (`role`, `permission_code`)
VALUES ('su', '*');
