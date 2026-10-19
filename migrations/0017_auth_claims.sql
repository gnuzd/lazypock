-- 0017_auth_claims.sql
--
-- `@request.auth.<field>` needs the authenticated record's own columns.
--
-- `auth.claim(name)` used to read only `request.auth.<name>`, and nothing ever set
-- anything but `id`, `email`, `collection_id` and `collection_name`. So a rule like
-- `@request.auth.role = 'admin'` — the documented shape, and the one the file rules
-- docs use — compiled, became a policy, and then evaluated to NULL for every
-- request: the rule was stored, displayed and silently never true.
--
-- The request context now publishes the record's columns as one JSON document under
-- `request.auth.claims`, and this function reads a claim out of it. The old
-- per-setting lookup stays as a fallback, so the settings that are already published
-- (`email`, and anything a future call site sets) keep resolving.
--
-- `create or replace` is enough: the generated policies call `auth.claim` by name,
-- so replacing the body takes effect without rebuilding them.

create or replace function auth.claim(name text) returns text
    language sql
    stable
    as $$
        select coalesce(
            nullif(current_setting('request.auth.claims', true), '')::jsonb ->> name,
            nullif(current_setting('request.auth.' || name, true), '')
        )
    $$;
