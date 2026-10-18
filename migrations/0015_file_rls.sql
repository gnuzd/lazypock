-- 0015_file_rls.sql
--
-- Row-level security for the media library (SECURITY_ROADMAP.md F7).
--
-- `_files` and `_file_refs` were the last hand-enforced surface: `files.rules`
-- were applied in Rust, so a query that forgot the check read or deleted every
-- row. RLS makes the default deny and leaves the rule as data.
--
-- The policies themselves are *not* here. A rule is a filter compiled by the
-- same parser a collection rule uses, and a policy cannot take parameters, so
-- the SQL is generated from the `files` setting at boot and whenever that
-- setting changes (`lazypock_files::policy`). Enabling RLS without a policy is
-- the default deny, which is the correct state between the migration and the
-- first apply.
--
-- `_file_deletions` is deliberately left without RLS. The `_files_deleted`
-- trigger inserts into it while a *restricted* DELETE runs, and a policy there
-- would make every file delete fail. Nothing but the reaper reads it, and the
-- reaper runs as the connecting owner.

alter table _files enable row level security;
alter table _file_refs enable row level security;

comment on table _files is
    'The media library. RLS is enabled; policies are generated from files.rules by lazypock_files::policy.';
comment on table _file_refs is
    'Record references to media files. RLS is enabled with no policies: internal bookkeeping runs as the owner.';
