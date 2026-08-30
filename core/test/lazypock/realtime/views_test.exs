defmodule Lazypock.Realtime.ViewsTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Realtime.Views
  alias Lazypock.Collections.Registry

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  setup do
    src = cname("src")
    view = cname("view")

    {:ok, _} =
      DDL.create_collection(src,
        type: "base",
        fields: [
          %{"name" => "title", "type" => "text", "required" => false},
          %{"name" => "count", "type" => "number", "required" => false}
        ]
      )

    {:ok, _} =
      DDL.create_collection(view,
        type: "view",
        options: %{"view_query" => "SELECT id, title FROM #{src}"}
      )

    Registry.reload!()
    {:ok, src: src, view: view}
  end

  describe "after_mutation/1" do
    test "broadcasts create events for rows that appear in a view", %{src: src, view: view} do
      Phoenix.PubSub.subscribe(Lazypock.PubSub, "collection:#{view}")
      Phoenix.PubSub.subscribe(Lazypock.PubSub, "collection:#{view}:*")

      # First mutation seeds the snapshot — no broadcasts for pre-existing rows.
      {:ok, _} = GenericRecord.insert(src, %{"title" => "seed", "count" => 1})
      Views.after_mutation(src)
      refute_receive %Phoenix.Socket.Broadcast{event: "record_change"}, 50

      # Second mutation diff: the new row should produce a create event.
      {:ok, record} = GenericRecord.insert(src, %{"title" => "new", "count" => 2})
      Views.after_mutation(src)

      assert_receive %Phoenix.Socket.Broadcast{
                       event: "record_change",
                       payload: %{action: "create", record: changed}
                     },
                     100

      assert changed["title"] == "new"
      assert changed["id"] == record["id"]
      assert changed["collectionName"] == view
    end

    test "broadcasts update events for changed rows", %{src: src, view: view} do
      {:ok, record} = GenericRecord.insert(src, %{"title" => "before", "count" => 1})
      Views.after_mutation(src)

      Phoenix.PubSub.subscribe(Lazypock.PubSub, "collection:#{view}")

      updated = GenericRecord.update(src, record["id"], %{"title" => "after"})
      Views.after_mutation(src)

      assert_receive %Phoenix.Socket.Broadcast{
                       event: "record_change",
                       payload: %{action: "update", record: changed}
                     },
                     100

      assert changed["id"] == updated["id"]
      assert changed["title"] == "after"
    end

    test "broadcasts delete events for removed rows", %{src: src, view: view} do
      {:ok, record} = GenericRecord.insert(src, %{"title" => "doomed", "count" => 1})
      Views.after_mutation(src)

      Phoenix.PubSub.subscribe(Lazypock.PubSub, "collection:#{view}")

      :ok = GenericRecord.delete(src, record["id"])
      Views.after_mutation(src)

      assert_receive %Phoenix.Socket.Broadcast{
                       event: "record_change",
                       payload: %{action: "delete", record: %{"id" => id}}
                     },
                     100

      assert id == record["id"]
    end

    test "does nothing when no views exist", %{src: src, view: view} do
      DDL.drop_collection(view)

      # No views registered — must not raise (and must not broadcast).
      Views.after_mutation(src)
      refute_receive %Phoenix.Socket.Broadcast{}, 50
    end
  end
end
