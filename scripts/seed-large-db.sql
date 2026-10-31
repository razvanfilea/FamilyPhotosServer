pragma foreign_keys=off;
delete from favorite_photos; delete from photos_hash; delete from photos_event_log; delete from folder_event_log;
delete from folder_permissions; delete from photos; delete from folders; delete from users;
insert into users values ('alice','Alice','x'),('bob','Bob','x'),('carol','Carol','x'),('test','Test','x');
-- folders: 1..100 public, 101..200 alice, 201..250 bob, 251..300 carol
with recursive n(i) as (select 1 union all select i+1 from n where i<300)
insert into folders (id, owner_id, name, created_at)
select i, case when i<=100 then null when i<=200 then 'alice' when i<=250 then 'bob' else 'carol' end,
  'folder ' || i, '2024-01-01 00:00:00' from n;
-- photos (:count rows over 15 years; half of the created_at values use the old 'YYYY-MM-DD HH:MM:SS' form)
with recursive n(i) as (select 1 union all select i+1 from n where i<:count),
r as (select i, (i*37) % 100 as u, (i*53 + i/100) % 100 as f, (i*2654435761) % (15*365*86400) as s from n)
insert into photos (id, user_id, name, created_at, file_size, folder_id, thumb_hash, trashed_on)
select i,
  case when u<40 then null when u<80 then 'alice' when u<90 then 'bob' else 'carol' end,
  'IMG_' || i || case when i % 50 = 0 then '.mp4' else '.jpg' end,
  case when i % 1000 = 3 then strftime('%Y-%m-%dT%H:%M:%S', 1320000000 + s, 'unixepoch') || '.250Z'
       when i % 2 = 0 then strftime('%Y-%m-%dT%H:%M:%SZ', 1320000000 + s, 'unixepoch')
       else strftime('%Y-%m-%d %H:%M:%S', 1320000000 + s, 'unixepoch') end,
  case when i % 100 = 0 then 60000000 + i else 2000000 + (i*7919) % 6000000 end,
  case when f < 70 then (case when u<40 then 1 + i % 100 when u<80 then 101 + i % 100 when u<90 then 201 + i % 50 else 251 + i % 50 end) else null end,
  randomblob(21),
  case when i % 100 = 7 then strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-' || (i % 45) || ' days') else null end
from r;
-- shares: bob shares 201..215 with alice (some can_delete), carol shares 251..260 (two expired)
insert into folder_permissions (folder_id, grantee_id, can_upload, can_delete, created_at, expires_at)
select id, 'alice', id % 2, id % 3 = 0, '2024-01-01 00:00:00', null from folders where id between 201 and 215;
insert into folder_permissions (folder_id, grantee_id, can_upload, can_delete, created_at, expires_at)
select id, 'alice', 0, 0, '2024-01-01 00:00:00', case when id <= 252 then '2024-06-01 00:00:00' else '2030-01-01T00:00:00Z' end from folders where id between 251 and 260;
insert into folder_permissions (folder_id, grantee_id, can_upload, can_delete, created_at) select id, 'bob', 1, 1, '2024-01-01 00:00:00' from folders where id between 101 and 110;
insert into favorite_photos select 'alice', id from photos where id % 25 = 0;
insert into favorite_photos select 'bob', id from photos where id % 40 = 0;
insert into photos_hash (photo_id, hash) select id, case when id % 200 = 1 then x'00112233445566778899aabbccddeeff' || randomblob(0) else randomblob(16) end from photos;
insert into photos_event_log (photo_id, user_id, data) select id, user_id, randomblob(200) from photos where id > :count - 512;
insert into folder_event_log (folder_id, photo_id, data) select folder_id, id, randomblob(200) from photos where folder_id is not null and id > :count / 5 * 4;
pragma foreign_keys=on;
analyze;
