CREATE TABLE folder_permissions
(
    id          INTEGER  NOT NULL PRIMARY KEY,
    owner_id    TEXT     NOT NULL,
    folder_name TEXT     NOT NULL,
    grantee_id  TEXT,
    token       TEXT UNIQUE,
    can_upload  BOOLEAN  NOT NULL DEFAULT FALSE,
    can_delete  BOOLEAN  NOT NULL DEFAULT FALSE,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at  DATETIME,

    FOREIGN KEY (owner_id) REFERENCES users (id) ON DELETE CASCADE,
    FOREIGN KEY (grantee_id) REFERENCES users (id) ON DELETE CASCADE,

    CHECK (grantee_id IS NOT NULL OR token IS NOT NULL),
    CHECK (expires_at > created_at)
);

CREATE UNIQUE INDEX idx_folder_permissions_token ON folder_permissions (token) WHERE token IS NOT NULL;
CREATE INDEX idx_folder_permissions_grantee ON folder_permissions (grantee_id) WHERE grantee_id IS NOT NULL;
CREATE INDEX idx_folder_permissions_owner ON folder_permissions (owner_id);
CREATE UNIQUE INDEX idx_folder_permissions_unique_grantee
    ON folder_permissions (owner_id, folder_name, grantee_id) WHERE grantee_id IS NOT NULL;
