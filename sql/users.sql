CREATE TABLE `users` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `status` tinyint(4) NOT NULL DEFAULT '0' comment '状态，0：禁用，1：启用',
  `created_at` timestamp NOT NULL comment '创建时间',
  `updated_at` timestamp NOT NULL comment '更新时间',
  `account` varchar(255) COLLATE utf8mb4_bin NOT NULL comment '账号',
  `password` varchar(255) COLLATE utf8mb4_bin NOT NULL comment '密码',
  `roles` json DEFAULT NULL comment '用户角色',
  `groups` json DEFAULT NULL comment '用户群组',
  `remark` varchar(255) COLLATE utf8mb4_bin DEFAULT NULL comment '备注',
  `email` varchar(255) COLLATE utf8mb4_bin DEFAULT NULL comment '用户邮箱',
  PRIMARY KEY (`id`) comment '主键',
  UNIQUE KEY `user_account` (`account`)
) ENGINE=InnoDB AUTO_INCREMENT=1 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;