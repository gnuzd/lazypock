defmodule PostHooks do
  use Lazypock.Hooks.Lifecycle, collection: "posts"

  @impl true
  def on_create(record, _context) do
    # Auto-generate slug from title
    slug = record["title"]
           |> to_string()
           |> String.downcase()
           |> String.replace(~r/[^a-z0-9]+/, "-")
           |> String.trim("-")

    {:ok, Map.put(record, "slug", slug)}
  end

  @impl true
  def after_create(_record, _context) do
    # Log the creation (fire-and-forget)
    :ok
  end
end
