CREATE TABLE folders
(
    id         INTEGER  NOT NULL PRIMARY KEY,
    owner_id   TEXT,
    name       TEXT     NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,

    FOREIGN KEY (owner_id) REFERENCES users (id) ON DELETE CASCADE,

    CHECK (name != '')
);

CREATE UNIQUE INDEX idx_folders_owner_name ON folders (COALESCE(owner_id, ''), name);

-- Populate the folders from the existing photo data
INSERT INTO folders (owner_id, name)
SELECT DISTINCT user_id, folder FROM photos
WHERE folder IS NOT NULL AND folder != '';

ALTER TABLE photos ADD COLUMN folder_id INTEGER REFERENCES folders (id) ON DELETE SET NULL;

UPDATE photos SET folder_id = (
    SELECT f.id FROM folders f
    WHERE COALESCE(f.owner_id, '') = COALESCE(photos.user_id, '')
      AND f.name = photos.folder
) WHERE folder IS NOT NULL AND folder != '';

ALTER TABLE photos DROP COLUMN folder;

-- Force client full-sync (schema payload changed to folder_id)
DELETE FROM photos_event_log;

CREATE TABLE folder_permissions
(
    id          INTEGER  NOT NULL PRIMARY KEY,
    folder_id   INTEGER  NOT NULL,
    grantee_id  TEXT,
    token       TEXT UNIQUE,
    can_upload  BOOLEAN  NOT NULL DEFAULT FALSE,
    can_delete  BOOLEAN  NOT NULL DEFAULT FALSE,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at  DATETIME,

    FOREIGN KEY (folder_id) REFERENCES folders (id) ON DELETE CASCADE,
    FOREIGN KEY (grantee_id) REFERENCES users (id) ON DELETE CASCADE,

    CHECK (grantee_id IS NOT NULL OR token IS NOT NULL),
    CHECK (expires_at IS NULL OR expires_at > created_at)
);

CREATE INDEX idx_folder_permissions_folder ON folder_permissions (folder_id);
CREATE INDEX idx_folder_permissions_grantee ON folder_permissions (grantee_id) WHERE grantee_id IS NOT NULL;
CREATE UNIQUE INDEX idx_folder_permissions_unique_grantee
    ON folder_permissions (folder_id, grantee_id) WHERE grantee_id IS NOT NULL;
