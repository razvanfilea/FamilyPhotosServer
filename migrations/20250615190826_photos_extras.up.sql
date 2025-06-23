CREATE TABLE photos_extras
(
    id        INTEGER NOT NULL PRIMARY KEY,
    sha       TEXT    NOT NULL,
    exif_json TEXT,

    FOREIGN KEY (id) REFERENCES photos (id) ON DELETE CASCADE
);