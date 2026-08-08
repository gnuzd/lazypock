defmodule PostHooks do
  @moduledoc """
  Example LazyPock hook using the PocketBase-parity event API.

  Mirrors `onRecordCreate((e) => { e.record.slug = ...; e.next() })` from
  https://pocketbase.io/docs/js-event-hooks/ — the handler receives an event
  `e`, mutates it, then calls `Lazypock.Hooks.Event.next(e)` to continue the
  chain. Returning `{:error, reason}` stops the chain.
  """
  use Lazypock.Hooks.Hook, collection: "posts"

  alias Lazypock.Hooks.Event
  require Logger

  # PocketBase: onRecordCreate
  def on_record_create(%Event{} = e) do
    # Auto-generate slug from title
    slug =
      e.record["title"]
      |> to_string()
      |> String.downcase()
      |> String.replace(~r/[^a-z0-9]+/, "-")
      |> String.trim("-")

    e = Event.put(e, :record, Map.put(e.record, "slug", slug))
    Event.next(e)
  end

  # PocketBase: onRecordAfterCreateSuccess
  def on_record_after_create_success(%Event{} = e) do
    # Log the creation (fire-and-forget)
    Logger.info("post created: #{e.record["id"]}")
    Event.next(e)
  end
end
