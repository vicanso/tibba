-- Trigger function: auto-update `modified` column on every UPDATE.
-- Must be created before any table that uses it.
CREATE OR REPLACE FUNCTION trigger_set_modified_timestamp()
RETURNS TRIGGER AS $$
BEGIN
  NEW.modified = CURRENT_TIMESTAMP;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

\i create_users.sql
\i create_configurations.sql
\i create_files.sql
\i create_http_detectors.sql
\i create_http_stats.sql
\i create_web_page_detectors.sql
\i create_objects.sql
\i create_detector_groups.sql
\i create_detector_group_users.sql
