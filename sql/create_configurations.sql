CREATE TABLE `configurations` (
  `id` BIGINT unsigned NOT NULL AUTO_INCREMENT COMMENT '主键ID',
  `status` TINYINT NOT NULL DEFAULT '0' COMMENT '状态，0：禁用，1：启用',
  `category` VARCHAR(50) NOT NULL COMMENT '配置类型',
  `name` VARCHAR(100) NOT NULL COMMENT '配置名称',
  `data` JSON NOT NULL COMMENT '配置数据',
  `description` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '配置描述',
  `effective_start_time` DATETIME NOT NULL COMMENT '生效开始时间',
  `effective_end_time` DATETIME NOT NULL COMMENT '生效结束时间',
  `created` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `modified` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `deleted_at` DATETIME DEFAULT NULL COMMENT '软删除时间',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_category_name` (`category`,`name`, `deleted_at`) COMMENT '配置类型和名称唯一索引（仅对未删除记录生效）',
  KEY `idx_effective_time` (`status`, `effective_start_time`, `effective_end_time`, `deleted_at`) COMMENT '状态和生效时间索引（包含软删除）'
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='系统配置表';