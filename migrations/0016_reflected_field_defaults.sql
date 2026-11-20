-- 0016_reflected_field_defaults.sql
--
-- Mark the fields of a reflected table as reflected.
--
-- Reflection records what Postgres reported as a column's default — `"0"`,
-- `nextval('audit_log_entry_id_seq'::regclass)` — in `_fields.options.default`.
-- That is SQL text, not the JSON value the declarative field `default` takes, so
-- validating it as one failed the moment a reflected collection was saved through
-- the Studio ("a number field's default must be a finite number, got \"0\"").
--
-- The field-level marker is what lets the declarative path skip it: a reflected
-- default belongs to the database, and the server must leave it alone rather than
-- try to re-express it. `options.default` is kept, because it is still what the
-- column does when a write omits it.
--
-- Idempotent, and only touches fields that do not carry the marker already.

update _fields
set options = options || '{"reflected": true}'::jsonb
where not (options ? 'reflected')
  and collection_id in (
    select id from _collections where options ->> 'reflected' = 'true'
  );
