ALTER TABLE photos ADD COLUMN thumb_hash BLOB;

-- Clear event log
DELETE FROM photos_event_log;
