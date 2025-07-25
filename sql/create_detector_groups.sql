CREATE TABLE `detector_groups` (
  `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT COMMENT '主键ID',
  `name` VARCHAR(255) NOT NULL COMMENT '组名称',
  `code` VARCHAR(100) NOT NULL COMMENT '组代码，用于程序标识',
  `owner_id` BIGINT UNSIGNED NOT NULL COMMENT '组所有者ID',
  `status` TINYINT NOT NULL DEFAULT '1' COMMENT '状态，0：禁用，1：启用',
  `remark` VARCHAR(1000) NOT NULL DEFAULT '' COMMENT '备注',
  `created` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `created_by` BIGINT UNSIGNED NOT NULL COMMENT '创建人',
  `modified` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `deleted_at` DATETIME DEFAULT NULL COMMENT '软删除时间',
  PRIMARY KEY (`id`) COMMENT '主键',
  KEY `idx_deleted_at` (`deleted_at`) COMMENT '软删除索引',
  KEY `idx_owner_id` (`owner_id`) COMMENT '所有者索引',
  UNIQUE KEY `uk_code` (`code`, `deleted_at`) COMMENT '组代码唯一索引（仅对未删除记录生效）'
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT="检测器组表";