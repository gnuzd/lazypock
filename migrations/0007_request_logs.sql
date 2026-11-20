-- Align `_logs` with the request-log API.
--
-- `_logs` has existed since 0001 but nothing ever wrote to it, and it was missing
-- the referer, the collection a request touched, and the (redacted) request body
-- the Studio's log detail shows. Studio also reads `duration` and `created_at`;
-- the handler maps the stored `duration_ms`/`created` in its SELECT rather than
-- renaming the columns, so the names this table shipped with stay put.

alter table _logs add column if not exists referer    text;
alter table _logs add column if not exists collection text;
alter table _logs add column if not exists body       text;

create index if not exists _logs_collection_idx on _logs (collection);
