CREATE TABLE `files` (
    `id` BIGINT unsigned NOT NULL AUTO_INCREMENT COMMENT '主键ID',
    `filename` VARCHAR(255) NOT NULL COMMENT '文件名',
    `file_size` BIGINT NOT NULL COMMENT '文件大小',
    `content_type` VARCHAR(100) NOT NULL COMMENT '内容类型',
    `group` VARCHAR(100) NOT NULL COMMENT '分组',
    `uploader` VARCHAR(100) NOT NULL COMMENT '上传者',
    
    -- 图片特有属性
    `image_width` INTEGER COMMENT '图片宽度',
    `image_height` INTEGER COMMENT '图片高度',
    
    -- 通用元数据
    `metadata` JSON COMMENT '存储其他元数据信息',
    
    `created` DATETIME DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
    `modified` DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
    PRIMARY KEY (`id`) COMMENT '主键',
    UNIQUE KEY `file_name` (`filename`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci COMMENT="文件表";
