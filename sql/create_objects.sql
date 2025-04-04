CREATE TABLE `objects` (
    `id` BIGINT NOT NULL AUTO_INCREMENT,
    `key` VARCHAR(2048) NOT NULL,        -- 对象路径
    `value` MEDIUMBLOB,                 -- 对象内容
    `modified` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP, -- 修改时间
    `created` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 创建时间

    PRIMARY KEY (`id`),
    UNIQUE KEY `idx_key` (`key`(768)),
    KEY `idx_modified` (`modified`),
    KEY `idx_created` (`created`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;