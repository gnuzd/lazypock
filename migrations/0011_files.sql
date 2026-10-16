-- The media library: uploaded files, their references, and their deletion queue.
--
-- `_files` is one row per stored object: what it is (name, size, mime, dimensions),
-- where it is (`storage_path` plus its generated `variants`) and who it belongs to
-- (`collection_name`/`record_id`/`field_name`, or nothing at all until it is
-- attached). Without it the server had no way to express an upload that is not part
-- of a record yet — the editor's and the library's uploads — could not report a
-- size, a dimension or an uploader, and could not refuse a delete while a record
-- still pointed at the file.
--
-- The id is the server's own text id, as in every other table: `_collections`,
-- `_fields` and records all use one, and the file id is opaque to a client, which
-- only ever holds it and hands it back.
--
-- Two tables follow the row through its life:
--
--   * `_file_refs` — which records embed a file (through a richtext/editor field).
--     It is what lets `DELETE /api/files/:id` refuse while something still points at
--     the file, and what lets the reaper reclaim an editor upload that was never
--     inserted into a record.
--   * `_file_deletions` — filled by a trigger on `_files`, **in the same
--     transaction** as the row's deletion, so every delete path (the API, a record
--     cascade, raw SQL) queues the object for removal. The worker drains it
--     afterwards: database first, storage second, so an object is never removed
--     while a row still points at it.

create table if not exists _files (
    id              text        primary key,
    filename        text        not null,
    extension       text        not null default '',
    mime_type       text        not null default 'application/octet-stream',
    size            bigint      not null default 0,
    storage_path    text        not null,
    storage_backend text        not null default 'local',
    -- Ownership. Empty strings rather than nulls: a file the library holds belongs
    -- to no record, which is a state, not a missing value.
    collection_name text        not null default '',
    record_id       text        not null default '',
    field_name      text        not null default '',
    -- Preset name => storage key, so the URLs are composed when they are served and a
    -- CDN or backend change does not invalidate what is stored.
    variants        jsonb       not null default '{}'::jsonb,
    -- `pending` until the upload is attached to a record, which is when the reaper
    -- stops considering it reclaimable.
    status          text        not null default 'ready'
                                check (status in ('pending', 'ready')),
    -- Where the upload came from: a record field, the richtext editor, or the media
    -- library itself. An editor or library upload may never be attached.
    origin          text        not null default 'field'
                                check (origin in ('field', 'editor', 'library')),
    original_name   text,
    width           integer,
    height          integer,
    checksum        text,
    uploaded_by     text        not null default '',
    attached_at     timestamptz,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now()
);

-- Every record delete asks for its files.
create index if not exists _files_record_idx on _files (collection_name, record_id);
-- The library lists newest first; the id tiebreak keeps paging stable when two
-- uploads share a timestamp.
create index if not exists _files_created_idx on _files (created_at desc, id desc);
-- The reaper's query: never-attached uploads, oldest first.
create index if not exists _files_pending_idx on _files (created_at) where status = 'pending';

create table if not exists _file_refs (
    file_id    text not null,
    collection text not null,
    record_id  text not null,
    field      text not null,
    primary key (file_id, collection, record_id, field)
);

create index if not exists _file_refs_file_idx on _file_refs (file_id);
create index if not exists _file_refs_record_idx on _file_refs (collection, record_id);

create table if not exists _file_deletions (
    id           bigserial   primary key,
    file_id      text        not null,
    backend      text        not null,
    storage_path text        not null,
    variants     jsonb       not null default '{}'::jsonb,
    attempts     integer     not null default 0,
    next_try_at  timestamptz not null default now(),
    created_at   timestamptz not null default now()
);

create index if not exists _file_deletions_due_idx on _file_deletions (next_try_at);

-- Queue the object whenever its row goes, whatever went first.
create or replace function _files_enqueue_deletion() returns trigger as $fn$
begin
    insert into _file_deletions (file_id, backend, storage_path, variants)
    values (old.id, old.storage_backend, old.storage_path, coalesce(old.variants, '{}'::jsonb));
    return old;
end;
$fn$ language plpgsql;

drop trigger if exists _files_deleted on _files;
create trigger _files_deleted
    after delete on _files
    for each row execute function _files_enqueue_deletion();
