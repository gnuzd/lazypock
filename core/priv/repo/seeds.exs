# LazyPock seed file — runs once after migrations (tracked in `_seeds_run`).
#
# This file is copied to `~/.lazypock/seeds.exs` on first boot and is then
# fully yours — edit it freely, add more seeds, re-run with `lazypock seed --force`.
#
# Inside the script you can read and write to any repository directly.
# Use the bang functions (`insert!`, `update!`) — they fail loudly on error.

alias Lazypock.Repo

# No bundled sample collections by default — the server ships with only the
# system collections (_superusers, users, ...). Add your own seeds below, e.g.:
#
#   if Repo.get_by(Lazypock.Collections.Collection, name: "posts") == nil do
#     {:ok, _collection} =
#       Lazypock.Schema.DDL.create_collection("posts",
#         type: "base",
#         fields: [
#           %{"name" => "title", "type" => "text", "required" => true}
#         ]
#       )
#   end
