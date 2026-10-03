defmodule Lazypock.Files.RefsTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Collections.Registry
  alias Lazypock.Files.Refs
  alias Lazypock.Files.Reaper
  alias Lazypock.Files.Store
  alias Lazypock.Repo
  alias Lazypock.Schema.DDL

  @collection "refs_posts"

  setup do
    {:ok, _} =
      DDL.create_collection(@collection,
        type: "base",
        fields: [
          %{"name" => "title", "type" => "text"},
          %{"name" => "body", "type" => "editor"}
        ]
      )

    Registry.reload!()
    :ok
  end

  defp store_image! do
    {:ok, file} = Store.store(Lazypock.TestImage.tiny_png!(40, 40), "img.png", [])
    file
  end

  describe "extract/1" do
    test "finds file ids in markdown, images and links" do
      id = "6f1c9d4e-1a2b-4c3d-8e9f-0a1b2c3d4e5f"

      assert Refs.extract("![alt](/api/files/#{id}/scale/content)") == [id]
      assert Refs.extract("<img src=\"https://cdn.test/lazypock/#{id}/content.webp\">") == [id]
      assert Refs.extract("text") == []
      assert Refs.extract(nil) == []
      assert Refs.extract("dup #{id} and #{id}") == [id]
    end
  end

  describe "sync_record/3" do
    test "adds references and marks the file attached" do
      file = store_image!()
      body = "![x](/api/files/#{file["id"]}/scale/content)"

      assert %{added: 1, removed: 0} = Refs.sync_record(@collection, "r1", %{"body" => body})

      assert [%{"field" => "body", "collection" => @collection, "recordId" => "r1"}] =
               Refs.usage(file["id"])

      assert Refs.referenced?(file["id"])

      assert {:ok, stored} = Store.get(file["id"])
      assert stored["attached_at"] != nil
    end

    test "removes references that are no longer present" do
      file = store_image!()
      Refs.sync_record(@collection, "r1", %{"body" => "![x](/api/files/#{file["id"]})"})
      assert Refs.referenced?(file["id"])

      assert %{added: 0, removed: 1} =
               Refs.sync_record(@collection, "r1", %{"body" => "no files"})

      refute Refs.referenced?(file["id"])
    end

    test "delete_record/2 drops all refs for the record" do
      file = store_image!()
      Refs.sync_record(@collection, "r1", %{"body" => "![x](/api/files/#{file["id"]})"})

      assert :ok = Refs.delete_record(@collection, "r1")
      refute Refs.referenced?(file["id"])
    end
  end

  describe "gc_unattached/1" do
    test "deletes editor uploads that were never attached, keeping library files" do
      editor = store_image!()
      library = store_image!()

      Ecto.Adapters.SQL.query!(
        Repo,
        "UPDATE _files SET origin = 'editor' WHERE id = $1",
        [Ecto.UUID.dump!(editor["id"])]
      )

      Ecto.Adapters.SQL.query!(
        Repo,
        "UPDATE _files SET origin = 'library' WHERE id = $1",
        [Ecto.UUID.dump!(library["id"])]
      )

      assert Refs.gc_unattached(0) == 1
      assert {:error, :not_found} = Store.get(editor["id"])
      assert {:ok, _} = Store.get(library["id"])

      Reaper.drain()
    end

    test "keeps attached files" do
      file = store_image!()
      Refs.sync_record(@collection, "r1", %{"body" => "![x](/api/files/#{file["id"]})"})

      Ecto.Adapters.SQL.query!(Repo, "UPDATE _files SET origin = 'editor' WHERE id = $1", [
        Ecto.UUID.dump!(file["id"])
      ])

      assert Refs.gc_unattached(0) == 0
      assert {:ok, _} = Store.get(file["id"])
    end
  end

  describe "rewrite_urls/3" do
    test "counts matches in dry-run and rewrites once" do
      file = store_image!()
      body = "![x](/api/files/#{file["id"]}/scale/content) and ![y](/api/files/other)"

      Ecto.Adapters.SQL.query!(
        Repo,
        "INSERT INTO #{@collection} (body) VALUES ($1)",
        [body]
      )

      assert %{matched: 1, updated: 0, dry_run: true} =
               Refs.rewrite_urls("/api/files", "https://cdn.test/f", dry_run: true)

      assert %{updated: 1, matched: 0} = Refs.rewrite_urls("/api/files", "https://cdn.test/f")

      assert %{updated: 0} = Refs.rewrite_urls("/api/files", "https://cdn.test/f")

      {:ok, %{rows: [[rewritten]]}} =
        Ecto.Adapters.SQL.query(Repo, "SELECT body FROM #{@collection} LIMIT 1", [])

      assert rewritten =~ "https://cdn.test/f/#{file["id"]}/scale/content"
      refute rewritten =~ "/api/files"
    end
  end
end
