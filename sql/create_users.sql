CREATE TABLE `users` (
  `id` bigint NOT NULL AUTO_INCREMENT,
  `status` tinyint NOT NULL DEFAULT '0' COMMENT '状态，0：禁用，1：启用',
  `created` TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) COMMENT '创建时间',
  `modified` TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3) COMMENT '更新时间',
  `account` varchar(255) NOT NULL,
  `password` varchar(255) NOT NULL COMMENT '密码',
  `roles` json DEFAULT NULL COMMENT '用户角色',
  `groups` json DEFAULT NULL COMMENT '用户群组',
  `remark` varchar(255) DEFAULT NULL COMMENT '备注',
  `email` varchar(255) DEFAULT NULL COMMENT '用户邮箱',
  `avatar` varchar(1024) DEFAULT NULL COMMENT '用户头像',
  PRIMARY KEY (`id`) COMMENT '主键',
  UNIQUE KEY `user_account` (`account`)
) ENGINE=InnoDB AUTO_INCREMENT=5 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
