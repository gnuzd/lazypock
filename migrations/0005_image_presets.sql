-- Image presets: seed the default sizes.
--
-- The table itself is created by `0001_system_tables.sql`, whose `fit` and
-- `format` vocabularies are the ones the API speaks. This migration only seeds
-- rows and opens the table for reading.

-- Seeded with `on conflict do nothing`, so a re-run neither fails nor overwrites
-- a size an operator has already tuned.
--
-- `thumb` is a square cover crop, for a grid tile. The others are `inside`, which
-- fits within the box and never enlarges: a small source keeps its own
-- resolution instead of being blown up into something blurry. A grid tile has its
-- own preset rather than scaling a smaller one, so it stays sharp.
insert into _image_presets (name, width, height, fit, format, quality) values
    ('thumb', 200, 200, 'cover', 'webp', 80),
    ('sm', 320, 320, 'inside', 'webp', 80),
    ('md', 768, 768, 'inside', 'webp', 80),
    ('lg', 1600, 1600, 'inside', 'webp', 80)
on conflict (name) do nothing;

-- Presets are configuration, like `_settings`: readable by anyone who can reach
-- the API, so a client can discover the available sizes. Writing one is a
-- superuser action, enforced in the API rather than by a policy here.
alter table _image_presets enable row level security;

drop policy if exists image_presets_read on _image_presets;
create policy image_presets_read on _image_presets for select using (true);
