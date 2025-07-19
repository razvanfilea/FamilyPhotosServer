PRAGMA defer_foreign_keys = ON;

CREATE TABLE photos_new
(
    id         INTEGER  NOT NULL PRIMARY KEY,
    user_id    TEXT,
    name       TEXT     NOT NULL,
    created_at DATETIME NOT NULL,
    file_size  INTEGER  NOT NULL,
    folder     TEXT,

    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);

INSERT INTO photos_new (id, user_id, name, created_at, file_size, folder)
SELECT id,
       CASE
           WHEN user_id = 'public' THEN NULL
           ELSE user_id
           END,
       name,
       created_at,
       file_size,
       folder
FROM photos;

DROP TABLE photos;

ALTER TABLE photos_new RENAME TO photos;

CREATE INDEX idx_photos_user_created_at_desc ON photos (user_id, created_at DESC);

DELETE FROM favorite_photos WHERE user_id = 'public';
DELETE FROM users WHERE id = 'public';

PRAGMA defer_foreign_keys = OFF;
