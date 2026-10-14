-- Undo checkpoints for import/restore.
--
-- A snapshot is the single-document JSON backup (`GET /api/export`'s shape) taken
-- immediately before an import, kept so a bad import can be rolled back. Only the
-- most recent one is kept: the Studio's action is "Undo last import", and a jsonb
-- copy of the whole database per import is not something to accumulate.
--
-- The payload is jsonb rather than a file, matching the original table; a large
-- database pays for that in the `_settings`-style row size, which is the tradeoff
-- the reference made too.

create table if not exists _import_snapshots (
    id         text        primary key,
    created_at timestamptz not null default now(),
    payload    jsonb       not null
);

create index if not exists _import_snapshots_created_idx
    on _import_snapshots (created_at desc);
