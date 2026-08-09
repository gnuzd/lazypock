# LazyPock seed file — runs once after migrations (tracked in `_seeds_run`).
#
# This file is copied to `~/.lazypock/seeds.exs` on first boot and is then
# fully yours — edit it freely, add more seeds, re-run with `lazypock seed --force`.
#
# Inside the script you can read and write to any repository directly.
# Use the bang functions (`insert!`, `update!`) — they fail loudly on error.

alias Lazypock.Repo

# ── Example: create a sample "posts" collection if it doesn't exist ──────────
if Repo.get_by(Lazypock.Collections.Collection, name: "posts") == nil do
  Repo.insert!(%Lazypock.Collections.Collection{
    name: "posts",
    type: "base",
    schema: [],
    rules: %{},
    options: %{},
    hooks: %{},
    managed: false
  })

  IO.puts("Seeded collection: posts")
else
  IO.puts("Collection 'posts' already exists — skipping")
end
