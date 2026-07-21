defmodule TestPostHooks do
  use Lazypock.Hooks.Lifecycle, collection: "test_posts"

  @impl true
  def on_create(record, _context) do
    IO.puts("  HOOK: on_create called with title=#{record["title"]}")
    {:ok, Map.put(record, "slug", "auto-generated-slug")}
  end

  @impl true
  def after_create(record, _context) do
    IO.puts("  HOOK: after_create for id=#{record["id"]}")
    :ok
  end
end
