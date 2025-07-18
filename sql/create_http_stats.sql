CREATE TABLE `http_stats` (
  `id` BIGINT unsigned NOT NULL AUTO_INCREMENT COMMENT '主键ID',
  `target_id` BIGINT unsigned NOT NULL COMMENT '目标ID',
  `target_name` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '目标名称',
  `url` TEXT NOT NULL COMMENT 'URL',
  `dns_lookup` INT NOT NULL DEFAULT -1 COMMENT 'DNS查询时间',
  `quic_connect` INT NOT NULL DEFAULT -1 COMMENT 'QUIC连接时间',
  `tcp_connect` INT NOT NULL DEFAULT -1 COMMENT 'TCP连接时间',
  `tls_handshake` INT NOT NULL DEFAULT -1 COMMENT 'TLS握手时间',
  `server_processing` INT NOT NULL DEFAULT -1 COMMENT '服务器处理时间',
  `content_transfer` INT NOT NULL DEFAULT -1 COMMENT '内容传输时间',
  `total` INT NOT NULL DEFAULT -1 COMMENT '总时间',
  `addr` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '地址',
  `status_code` SMALLINT unsigned NOT NULL DEFAULT 0 COMMENT '状态码',
  `tls` VARCHAR(20) NOT NULL DEFAULT '' COMMENT 'TLS版本',
  `alpn` VARCHAR(10) NOT NULL DEFAULT '' COMMENT 'ALPN',
  `subject` VARCHAR(1000) NOT NULL DEFAULT '' COMMENT '证书主题',
  `issuer` VARCHAR(1000) NOT NULL DEFAULT '' COMMENT '证书颁发者',
  `cert_not_before` VARCHAR(32) NOT NULL DEFAULT '' COMMENT '证书有效期开始时间',
  `cert_not_after` VARCHAR(32) NOT NULL DEFAULT '' COMMENT '证书有效期结束时间',
  `cert_cipher` VARCHAR(50) NOT NULL DEFAULT '' COMMENT '证书加密套件',
  `cert_domains` VARCHAR(3000) NOT NULL DEFAULT '' COMMENT '证书域名',
  `body_size` INT NOT NULL DEFAULT -1 COMMENT '响应体大小',
  `region` VARCHAR(64) NOT NULL DEFAULT '' COMMENT '触发区域',
  `error` TEXT COMMENT '错误信息',
  `result` TINYINT unsigned NOT NULL DEFAULT 0 COMMENT '结果',
  `remark` VARCHAR(1000) NOT NULL DEFAULT '' COMMENT '备注',
  `created` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `modified` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `deleted_at` DATETIME DEFAULT NULL COMMENT '软删除时间',
  PRIMARY KEY (`id`) COMMENT '主键',
  KEY `idx_deleted_at` (`deleted_at`) COMMENT '软删除索引',
  KEY `idx_target_id_result` (`target_id`, `result`) COMMENT '目标ID和结果索引',
  KEY `idx_modified` (`modified`) COMMENT '更新时间索引'
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT="http_stats表";
