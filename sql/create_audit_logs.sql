CREATE TABLE `audit_logs` (
  `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT COMMENT '主键ID',
  `user_id` BIGINT UNSIGNED DEFAULT NULL COMMENT '操作主体；NULL 表示匿名 / 系统级操作',
  `action` VARCHAR(64) NOT NULL COMMENT '操作类型，约定 "{resource}.{action}" 形如 user.login',
  `target_type` VARCHAR(64) NOT NULL DEFAULT '' COMMENT '操作目标类型，如 user / file / permission',
  `target_id` VARCHAR(64) NOT NULL DEFAULT '' COMMENT '操作目标 id 字符串',
  `detail` JSON NOT NULL COMMENT '自由结构化补充信息（前后值、provider、命中规则等）',
  `request_id` VARCHAR(128) NOT NULL DEFAULT '' COMMENT '关联 X-Request-ID，串联请求链路',
  `ip` VARCHAR(64) NOT NULL DEFAULT '' COMMENT '客户端 IP',
  `user_agent` VARCHAR(255) NOT NULL DEFAULT '' COMMENT 'User-Agent 截断 255 字符',
  `created` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  PRIMARY KEY (`id`) COMMENT '主键',
  KEY `idx_user` (`user_id`, `created` DESC) COMMENT '按用户看审计史',
  KEY `idx_action` (`action`, `created` DESC) COMMENT '按动作类型聚合',
  KEY `idx_request` (`request_id`) COMMENT '串联请求链路',
  KEY `idx_created` (`created` DESC) COMMENT '按时间扫描'
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT="审计日志：关键操作的「谁、何时、做了什么」";
