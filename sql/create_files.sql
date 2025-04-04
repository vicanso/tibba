CREATE TABLE `files` (
    `id` BIGINT NOT NULL AUTO_INCREMENT,
    `filename` VARCHAR(255) NOT NULL,
    `file_size` BIGINT NOT NULL,
    `content_type` VARCHAR(100) NOT NULL,
    `bucket` VARCHAR(100) NOT NULL,
    
    -- 图片特有属性
    `image_width` INTEGER,
    `image_height` INTEGER,
    
    -- 通用元数据
    `metadata` JSON,               -- 存储其他元数据信息
    
    `created` DATETIME DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
    `modified` DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
    PRIMARY KEY (`id`) COMMENT '主键',
    UNIQUE KEY `file_name` (`filename`)
) ENGINE=InnoDB AUTO_INCREMENT=5 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
