defmodule Lazypock.Repo.Migrations.AlignColumnsAndSystemTimestamps do
  use Ecto.Migration

  @moduledoc """
  Two schema-alignment fixes for existing databases.

  1. **Verbatim column names.** Field names are kept verbatim end to end
     (`field_name` stays `field_name`, `fieldName` stays `fieldName`), and
     columns are declared with quoted identifiers so PostgreSQL preserves the
     case. Bundled migrations and older releases declared a few columns
     unquoted, so PostgreSQL folded them to lowercase (`emailvisibility`,
     `verificationtoken`). Rename any column whose name differs from its
     `_fields.name` only by case.

  2. **System timestamps on every collection.** `created_at`/`updated_at` have
     always existed as physical columns, but base/auth collections created
     through the API/Studio never got `_fields` metadata rows for them, so the
     collection schema, the Studio field list and the generated SDK types hid
     them. Add the missing columns + metadata rows (system `autodate`) and
     rebuild each collection's `schema` JSON from `_fields`.

  Idempotent — safe to re-run.
  """

  # Shared FROM/WHERE fragment: base/auth collections whose physical table
  # actually exists. A stale `_collections` row left behind by an aborted run
  # must not break the migration; the registration pass physicalizes those.
  @managed_collections_from """
  FROM _collections c
  WHERE c.type IN ('base', 'auth')
    AND EXISTS (
      SELECT 1 FROM information_schema.tables t
      WHERE t.table_schema = 'public'
        AND t.table_name = c.name
        AND t.table_type = 'BASE TABLE'
    )
  """

  def up do
    rename_lowercased_columns()
    ensure_system_timestamps()
    rebuild_schemas()
  end

  def down do
    # Column renames and backfilled system metadata are intentional one-way
    # fixes — there is no safe way to tell a pre-existing column apart from a
    # user field that happens to share the name.
    :ok
  end

  # Renames any physical column that matches its field metadata name
  # case-insensitively but not exactly (legacy unquoted DDL folded the case).
  # Views are skipped: their columns come from the view query.
  defp rename_lowercased_columns do
    execute """
    DO $$
    DECLARE
      r RECORD;
      actual TEXT;
    BEGIN
      FOR r IN
        SELECT c.name AS coll, f.name AS fld
        FROM _collections c
        JOIN _fields f ON f.collection_id = c.id
        WHERE c.type <> 'view'
      LOOP
        SELECT column_name INTO actual
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = r.coll
          AND lower(column_name) = lower(r.fld)
        ORDER BY (column_name = r.fld) DESC
        LIMIT 1;

        IF actual IS NOT NULL AND actual <> r.fld THEN
          EXECUTE format('ALTER TABLE %I RENAME COLUMN %I TO %I', r.coll, actual, r.fld);
        END IF;
      END LOOP;
    END $$;
    """
  end

  # Guarantees every base/auth collection has both physical timestamp columns
  # and the matching system `autodate` field metadata.
  defp ensure_system_timestamps do
    execute """
    DO $$
    DECLARE
      r RECORD;
    BEGIN
      FOR r IN
        SELECT c.name #{@managed_collections_from}
      LOOP
        EXECUTE format(
          'ALTER TABLE %I ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT now()',
          r.name
        );
        EXECUTE format(
          'ALTER TABLE %I ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now()',
          r.name
        );
      END LOOP;
    END $$;
    """

    for {name, options} <- [
          {"created_at", ~s({"onCreate": true})},
          {"updated_at", ~s({"onCreate": true, "onUpdate": true})}
        ] do
      execute """
      INSERT INTO _fields (collection_id, name, type, required, hidden, system, sort_order, options, created_at, updated_at)
      SELECT c.id, '#{name}', 'autodate', true, false, true,
             COALESCE((SELECT max(f2.sort_order) + 1 FROM _fields f2 WHERE f2.collection_id = c.id), 0),
             '#{options}'::jsonb, now(), now()
      #{@managed_collections_from}
        AND NOT EXISTS (
          SELECT 1 FROM _fields f
          WHERE f.collection_id = c.id AND lower(f.name) = '#{name}'
        );
      """
    end
  end

  # Mirrors DDL.update_collection_schema!/1: rebuild the stored schema JSON
  # from the (authoritative) `_fields` rows so it matches the field list.
  # `DEFAULT` is stored as JSON-encoded TEXT (Lazypock.Ecto.JSONValue), so the
  # cast reproduces the decoded value the Elixir path writes.
  defp rebuild_schemas do
    execute """
    UPDATE _collections c
    SET schema = sub.schema
    FROM (
      SELECT f.collection_id,
             jsonb_agg(
               jsonb_build_object(
                 'name', f.name,
                 'type', f.type,
                 'required', f.required,
                 'unique', f.unique,
                 'default', CASE WHEN f.default_value IS NULL THEN NULL ELSE f.default_value::jsonb END,
                 'options', f.options,
                 'indexed', f.indexed
               )
               ORDER BY f.sort_order, f.name
             ) AS schema
      FROM _fields f
      GROUP BY f.collection_id
    ) sub
    WHERE c.id = sub.collection_id;
    """
  end
end
