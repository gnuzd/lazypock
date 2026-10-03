defmodule Lazypock.Files.DirectUploadTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Files.Adapters.S3
  alias Lazypock.Files.DirectUpload
  alias Lazypock.Files.Policy
  alias Lazypock.Files.Storage
  alias Lazypock.Files.Store
  alias Lazypock.Repo
  alias Lazypock.Settings

  @stub :lazypock_direct_stub
  @policy_defaults %{"max_megapixels" => 40, "max_dimension" => 10_000}

  setup do
    original = Settings.get()
    Storage.clear_cache()
    Application.put_env(:lazypock, S3, req_options: [plug: {Req.Test, @stub}])

    Settings.put(
      Map.put(Settings.get(), "storage", %{
        "backend" => "s3",
        "endpoint" => "https://s3.test",
        "region" => "auto",
        "bucket" => "bucket",
        "access_key_id" => "AKIA",
        "secret_access_key" => "secret",
        "prefix" => "lazypock/",
        "public_base_url" => "https://cdn.test"
      })
    )

    Storage.clear_cache()

    on_exit(fn ->
      Application.delete_env(:lazypock, S3)
      Settings.put(original)
      Storage.clear_cache()
    end)

    :ok
  end

  defp policy, do: Policy.resolve(@policy_defaults)

  defp stored_row(id) do
    {:ok, %{rows: [row], columns: cols}} =
      Ecto.Adapters.SQL.query(Repo, "SELECT * FROM _files WHERE id = $1", [Ecto.UUID.dump!(id)])

    cols |> Enum.zip(row) |> Map.new()
  end

  test "presign creates a pending row and a signed PUT URL" do
    {:ok, info} =
      DirectUpload.presign(
        %{"filename" => "photo.png", "size" => 1234, "mime" => "image/png"},
        policy()
      )

    assert info["method"] == "PUT"
    assert info["url"] =~ "/bucket/lazypock/#{info["id"]}/original.png?"
    assert info["url"] =~ "X-Amz-Signature="
    assert info["headers"]["content-length"] == "1234"

    row = stored_row(info["id"])
    assert row["status"] == "pending"
    assert row["storage_backend"] == "s3"
    assert row["storage_path"] == "lazypock/#{info["id"]}/original.png"
    assert row["origin"] == "library"
  end

  test "presign rejects oversized, disallowed and local-backend requests" do
    assert {:error, :too_large} =
             DirectUpload.presign(
               %{"filename" => "a.png", "size" => 999_999_999, "mime" => "image/png"},
               policy()
             )

    assert {:error, :invalid_mime_type} =
             DirectUpload.presign(
               %{"filename" => "a.png", "size" => 10, "mime" => "application/pdf"},
               Policy.resolve(%{"mime_types" => ["image/*"]})
             )

    assert {:error, :invalid_extension} =
             DirectUpload.presign(
               %{"filename" => "a.exe", "size" => 10, "mime" => "image/png"},
               policy()
             )

    Settings.put(Map.put(Settings.get(), "storage", %{"backend" => "local"}))
    Storage.clear_cache()

    assert {:error, :direct_upload_requires_s3} =
             DirectUpload.presign(
               %{"filename" => "a.png", "size" => 10, "mime" => "image/png"},
               policy()
             )
  end

  test "complete verifies the object, generates variants and marks it ready" do
    png = Lazypock.TestImage.tiny_png!(120, 80)

    {:ok, info} =
      DirectUpload.presign(
        %{"filename" => "photo.png", "size" => byte_size(png), "mime" => "image/png"},
        policy()
      )

    stub_complete(byte_size(png), png)

    assert {:ok, record} = DirectUpload.complete(info["id"], policy())
    assert record["status"] == "ready"
    assert record["width"] == 120
    assert record["height"] == 80
    assert Map.has_key?(record["variants"], "thumb")
    assert Map.has_key?(record["variants"], "content")

    assert stored_row(info["id"])["status"] == "ready"
  end

  test "complete rejects a size mismatch, deleting the object and the row" do
    {:ok, info} =
      DirectUpload.presign(
        %{"filename" => "photo.png", "size" => 500, "mime" => "image/png"},
        policy()
      )

    parent = self()

    Req.Test.stub(@stub, fn conn ->
      cond do
        conn.method == "HEAD" ->
          conn
          |> Plug.Conn.put_resp_header("content-length", "999")
          |> Plug.Conn.send_resp(200, "")

        conn.method == "DELETE" ->
          send(parent, {:deleted, conn.request_path})
          Plug.Conn.send_resp(conn, 204, "")

        conn.method == "GET" ->
          Plug.Conn.send_resp(conn, 200, "")
      end
    end)

    assert {:error, {:size_mismatch, 999, 500}} = DirectUpload.complete(info["id"], policy())
    assert {:error, :not_found} = Store.get(info["id"])
    assert_receive {:deleted, _path}
  end

  defp stub_complete(size, png) do
    Req.Test.stub(@stub, fn conn ->
      cond do
        conn.method == "HEAD" ->
          conn
          |> Plug.Conn.put_resp_header("content-length", to_string(size))
          |> Plug.Conn.send_resp(200, "")

        conn.method == "GET" and String.ends_with?(conn.request_path, "original.png") ->
          Plug.Conn.send_resp(conn, 200, png)

        conn.method == "GET" ->
          Plug.Conn.send_resp(conn, 404, "")

        conn.method == "PUT" ->
          {:ok, _body, conn} = Plug.Conn.read_body(conn)
          Plug.Conn.send_resp(conn, 200, "")

        true ->
          Plug.Conn.send_resp(conn, 404, "")
      end
    end)
  end
end
