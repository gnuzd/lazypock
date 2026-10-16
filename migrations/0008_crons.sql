-- Align `_crons` with the cron admin API.
--
-- The table shipped with `payload` and without the bookkeeping columns the Studio
-- reads. The reference implementation (and the Studio) call the action settings
-- `config`, and the cron page shows the last run's duration and error, so those
-- columns are added here. `payload` was never read by anything; it is dropped
-- rather than left as a second, silently-ignored place to put settings.

alter table _crons add column if not exists config           jsonb not null default '{}'::jsonb;
alter table _crons add column if not exists last_duration_ms integer;
alter table _crons add column if not exists last_error       text;

alter table _crons drop column if exists payload;

-- A job name is how an operator recognises it, so two jobs must not share one.
create unique index if not exists _crons_name_idx on _crons (name);
