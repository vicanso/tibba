CREATE TABLE `users` (
  `id` BIGINT unsigned NOT NULL AUTO_INCREMENT COMMENT '主键ID',
  `status` TINYINT NOT NULL DEFAULT '0' COMMENT '状态，0：禁用，1：启用',
  `account` VARCHAR(255) NOT NULL,
  `password` VARCHAR(255) NOT NULL COMMENT '密码',
  `roles` JSON NOT NULL DEFAULT (JSON_ARRAY()) COMMENT '用户角色',
  `groups` JSON NOT NULL DEFAULT (JSON_ARRAY()) COMMENT '用户群组',
  `remark` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '备注',
  `email` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '用户邮箱',
  `avatar` VARCHAR(1024) NOT NULL DEFAULT '' COMMENT '用户头像',
  `created` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `modified` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `deleted_at` DATETIME DEFAULT NULL COMMENT '软删除时间',
  PRIMARY KEY (`id`) COMMENT '主键',
  UNIQUE KEY `user_account` (`account`, `deleted_at`) COMMENT '用户账号唯一索引（仅对未删除记录生效）',
  KEY `idx_deleted_at` (`deleted_at`) COMMENT '软删除索引'
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci COMMENT="用户表";

