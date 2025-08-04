CREATE TABLE photos_event_log
(
    event_id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    photo_id INTEGER NOT NULL,
    user_id  TEXT,
    data     BLOB
);

CREATE INDEX idx_event_log_user_id ON photos_event_log (user_id);