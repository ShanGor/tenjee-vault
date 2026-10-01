-- M4: 全局页面模板及跨模块偏好。
-- 模板内容是 TipTap JSON；加密分区模板仍存放在各自的 space.db，不能写入本表。
CREATE TABLE page_templates (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL COLLATE NOCASE CHECK (length(trim(name)) BETWEEN 1 AND 120),
    content_json TEXT NOT NULL CHECK (json_valid(content_json)),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX idx_page_templates_name ON page_templates(name COLLATE NOCASE);

-- 这些值沿用既有 app_config 键值存储。INSERT OR IGNORE 既不覆盖用户设置，
-- 也使从 M3 升级与新建资料库得到同一组 v1 默认值。
INSERT OR IGNORE INTO app_config (key, value) VALUES
    ('auto_backup_enabled', 'false'),
    ('auto_backup_schedule', 'daily'),
    ('auto_backup_directory', ''),
    ('auto_backup_retention_count', '5'),
    ('auto_backup_last_success_at', ''),
    ('app_locale', 'system'),
    ('app_theme', 'system'),
    ('close_to_tray', 'true');

-- app_config 仍允许后续版本加入键；M4 已定义键的值在数据库边界受约束。
CREATE TRIGGER validate_m4_preference_insert
BEFORE INSERT ON app_config
WHEN (NEW.key IN ('auto_backup_enabled', 'close_to_tray')
      AND NEW.value NOT IN ('true', 'false'))
  OR (NEW.key = 'auto_backup_schedule'
      AND NEW.value NOT IN ('daily', 'weekly'))
  OR (NEW.key = 'auto_backup_retention_count'
      AND (NEW.value GLOB '*[^0-9]*' OR CAST(NEW.value AS INTEGER) NOT BETWEEN 1 AND 365))
  OR (NEW.key = 'app_locale'
      AND NEW.value NOT IN ('system', 'zh-CN', 'en'))
  OR (NEW.key = 'app_theme'
      AND NEW.value NOT IN ('system', 'light', 'dark'))
BEGIN
    SELECT RAISE(ABORT, 'invalid M4 preference value');
END;

CREATE TRIGGER validate_m4_preference_update
BEFORE UPDATE OF key, value ON app_config
WHEN (NEW.key IN ('auto_backup_enabled', 'close_to_tray')
      AND NEW.value NOT IN ('true', 'false'))
  OR (NEW.key = 'auto_backup_schedule'
      AND NEW.value NOT IN ('daily', 'weekly'))
  OR (NEW.key = 'auto_backup_retention_count'
      AND (NEW.value GLOB '*[^0-9]*' OR CAST(NEW.value AS INTEGER) NOT BETWEEN 1 AND 365))
  OR (NEW.key = 'app_locale'
      AND NEW.value NOT IN ('system', 'zh-CN', 'en'))
  OR (NEW.key = 'app_theme'
      AND NEW.value NOT IN ('system', 'light', 'dark'))
BEGIN
    SELECT RAISE(ABORT, 'invalid M4 preference value');
END;
