CREATE TABLE `settings` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `status` tinyint(4) NOT NULL DEFAULT '0' comment '状态，0：禁用，1：启用',
  `created_at` timestamp NOT NULL comment '创建时间',
  `updated_at` timestamp NOT NULL comment '更新时间',
  `name` varchar(255) COLLATE utf8mb4_bin NOT NULL comment '配置名称',
  `category` varchar(255) COLLATE utf8mb4_bin NOT NULL comment '配置类别',
  `data` longtext COLLATE utf8mb4_bin NOT NULL comment '配置数据',
  `remark` varchar(255) COLLATE utf8mb4_bin NOT NULL comment '配置备注',
  `started_at` timestamp NOT NULL comment '配置启用时间',
  `ended_at` timestamp NOT NULL comment '配置结束时间',
  `updater` varchar(255) COLLATE utf8mb4_bin DEFAULT '' comment '更新者',
  `creator` varchar(255) COLLATE utf8mb4_bin NOT NULL comment '创建者',
  PRIMARY KEY (`id`) comment '主键',
  UNIQUE KEY `setting_name` (`name`),
  KEY `setting_created_at` (`created_at`),
  KEY `setting_updated_at` (`updated_at`),
  KEY `setting_status_category` (`status`,`category`)
) ENGINE=InnoDB AUTO_INCREMENT=1 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;
