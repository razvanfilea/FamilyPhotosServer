-- Fix existing photos whose user_id doesn't match their folder's owner_id
update photos set user_id = (select owner_id from folders where id = photos.folder_id)
where folder_id is not null
  and user_id is not (select owner_id from folders where id = photos.folder_id);

-- Trigger: reject photo INSERT if user_id doesn't match folder owner
create trigger trg_photos_insert_owner_check
before insert on photos
when new.folder_id is not null
begin
    select raise(abort, 'photos.user_id must match folder.owner_id')
    where (select owner_id is not new.user_id from folders where id = new.folder_id);
end;

-- Trigger: reject photo UPDATE if user_id doesn't match folder owner
create trigger trg_photos_update_owner_check
before update of folder_id, user_id on photos
when new.folder_id is not null
begin
    select raise(abort, 'photos.user_id must match folder.owner_id')
    where (select owner_id is not new.user_id from folders where id = new.folder_id);
end;

-- Trigger: when folder owner changes, cascade to all photos in that folder
create trigger trg_folders_update_cascade_owner
after update of owner_id on folders
when old.owner_id is not new.owner_id
begin
    update photos set user_id = new.owner_id where folder_id = new.id;
end;

drop index idx_event_log_user_id;

-- Index for photos by folder (used by get_photos_in_folder, trigger cascade, get_photo_ids_in_folder)
create index idx_photos_folder_id on photos (folder_id) where folder_id is not null;

-- Index for hash lookups (get_photo_with_hash currently does full scan of photos)
create index idx_photos_hash_hash on photos_hash (hash);
