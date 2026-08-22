defmodule Lazypock.Repo.Migrations.RecreateUsersCollection do
  use Ecto.Migration

  @doc """
  The built-in `users` auth collection can be missing on instances that ran
  the pre-0.4.2 import `deleteMissing=true` path — users was registered with
  `system = false`, so the import drop loop could remove it. Recreate it
  idempotently (table, `_collections` row, `_fields` metadata) so existing
  deployments self-heal on upgrade. Mirrors the definitions from
  `setup_system_collections_proper` + `add_auth_collection_fields`.

  `users` is a **normal** (non-system) auth collection, matching PocketBase,
  so it is registered with `system = false` — only the internal `_` tables
  are system collections.
  """
  def up do
    execute """
    CREATE TABLE IF NOT EXISTS users (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      email TEXT UNIQUE NOT NULL,
      password_hash TEXT NOT NULL,
      avatar TEXT,
      name TEXT,
      verified BOOLEAN NOT NULL DEFAULT false,
      verificationToken TEXT,
      emailVisibility BOOLEAN NOT NULL DEFAULT true,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )
    """

    execute """
    INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
    SELECT 'users', 'auth', false, true, '[]',
      '{"listRule": "", "viewRule": "", "createRule": "", "updateRule": "id = @request.auth.id", "deleteRule": "id = @request.auth.id"}'::jsonb,
      '{}', '{}', now(), now()
    WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = 'users')
    """

    users_fields = [
      %{name: "email", type: "email", required: true, system: true, sort: 0,
        options: "{}"},
      %{name: "password_hash", type: "password", required: true, system: true, sort: 1,
        options: "{}"},
      %{name: "created_at", type: "autodate", required: true, system: true, sort: 2,
        options: ~s({"onCreate": true})},
      %{name: "updated_at", type: "autodate", required: true, system: true, sort: 3,
        options: ~s({"onCreate": true, "onUpdate": true})},
      %{name: "name", type: "text", required: false, system: false, sort: 4,
        options: ~s({"max": 255})},
      %{name: "avatar", type: "file", required: false, system: false, sort: 5,
        options:
          ~s({"maxSelect": 1, "mimeTypes": ["image/jpeg","image/png","image/svg+xml","image/gif","image/webp"]})},
      %{name: "verified", type: "bool", required: true, system: true, sort: 6,
        options: "{}"},
      %{name: "verificationToken", type: "text", required: false, system: true, sort: 7,
        options: "{}"},
      %{name: "emailVisibility", type: "bool", required: true, system: true, sort: 8,
        options: ~s({"defaultValue": true})}
    ]

    for f <- users_fields do
      execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
      SELECT c.id, '#{f.name}', '#{f.type}', #{f.required}, #{f.system}, #{f.sort}, '#{f.options}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = 'users'
      AND NOT EXISTS (SELECT 1 FROM _fields f2 WHERE f2.collection_id = c.id AND f2.name = '#{f.name}')
      """
    end
  end

  def down do
    # Recreating a missing system collection is one-way — no-op on down.
    :ok
  end
end
