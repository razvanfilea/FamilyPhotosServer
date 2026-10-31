-- Restores the index only. The rewrite of created_at to 'YYYY-MM-DDTHH:MM:SSZ' can't be undone
-- and doesn't need to be: both forms decode to the same instant.
drop index idx_photos_folder_created;

create index idx_photos_folder_id on photos (folder_id) where folder_id is not null;
