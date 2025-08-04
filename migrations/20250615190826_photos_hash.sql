CREATE TABLE photos_hash
(
    photo_id   INTEGER  NOT NULL PRIMARY KEY,
    hash       BLOB     NOT NULL,
    created_at DATETIME NOT NULL DEFAULT current_timestamp,

    FOREIGN KEY (photo_id) REFERENCES photos (id) ON DELETE CASCADE
);
