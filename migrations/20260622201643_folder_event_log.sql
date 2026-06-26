create table folder_event_log (
    event_id  integer not null primary key autoincrement,
    folder_id integer not null,
    photo_id  integer not null,
    data      blob,
    foreign key (folder_id) references folders (id) on delete cascade
);

create index idx_folder_event_log_folder on folder_event_log (folder_id, event_id);

