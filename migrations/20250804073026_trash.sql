ALTER TABLE photos ADD COLUMN trashed_on DATETIME;

-- Clear event log
DELETE FROM photos_event_log;