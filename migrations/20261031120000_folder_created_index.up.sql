-- Month queries compare created_at as text, so every row has to be UTC ISO text ending in 'Z'.
-- Rows written long ago use 'YYYY-MM-DD HH:MM:SS'.
update photos
set created_at = strftime('%Y-%m-%dT%H:%M:%SZ', created_at)
where created_at not like '%Z';

drop index idx_photos_folder_id;

create index idx_photos_folder_created on photos (folder_id, created_at desc, id desc) where folder_id is not null;
