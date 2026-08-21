# LazyPock seed file — runs once after migrations (tracked in `_seeds_run`).
#
# This file is copied to `~/.lazypock/seeds.exs` on first boot and is then
# fully yours — edit it freely, add more seeds, re-run with `lazypock seed --force`.
#
# Inside the script you can read and write to any repository directly.
# Use the bang functions (`insert!`, `update!`) — they fail loudly on error.

alias Lazypock.Repo

# ── Example: create a sample "posts" collection if it doesn't exist ──────────
# Use DDL.create_collection (NOT a raw Repo.insert!) so the collection gets a
# real Postgres table + field metadata and `managed: true` — otherwise it
# cannot be deleted and record queries fail with "relation posts does not exist".
if Repo.get_by(Lazypock.Collections.Collection, name: "posts") == nil do
  {:ok, _collection} =
    Lazypock.Schema.DDL.create_collection("posts",
      type: "base",
      fields: [
        %{"name" => "title", "type" => "text", "required" => true},
        # The bundled post_hooks.ex example auto-generates a slug on create
        %{"name" => "slug", "type" => "text"},
        %{"name" => "body", "type" => "editor"},
        %{"name" => "published", "type" => "bool", "default" => false}
      ]
    )

  IO.puts("Seeded collection: posts")
else
  IO.puts("Collection 'posts' already exists — skipping")
end
