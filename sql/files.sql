CREATE TABLE `files` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `created_at` timestamp NOT NULL comment '创建时间',
  `updated_at` timestamp NOT NULL comment '更新时间',
  `name` varchar(255) COLLATE utf8mb4_bin NOT NULL comment '文件名称',
  `size` bigint(20) NOT NULL comment '文件大小',
  `content_type` varchar(255) COLLATE utf8mb4_bin NOT NULL comment '文件类型',
  `data` mediumblob NOT NULL comment '文件数据',
  `updater` varchar(255) COLLATE utf8mb4_bin DEFAULT '' comment '更新者',
  `creator` varchar(255) COLLATE utf8mb4_bin NOT NULL comment '创建者',
  PRIMARY KEY (`id`),
  UNIQUE KEY `name` (`name`),
  UNIQUE KEY `file_name` (`name`),
  KEY `file_created_at` (`created_at`),
  KEY `file_updated_at` (`updated_at`)
) ENGINE=InnoDB AUTO_INCREMENT=1 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;
