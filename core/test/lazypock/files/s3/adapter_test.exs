defmodule Lazypock.Files.S3.AdapterTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Files.Adapters.S3
  alias Lazypock.Files.Storage
  alias Lazypock.Settings

  @stub :lazypock_s3_stub

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
        "access_key_id" => "AKIAIOSFODNN7EXAMPLE",
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

  defp stub(fun), do: Req.Test.stub(@stub, fun)

  test "store streams the object to <prefix><id>/original.<ext>" do
    parent = self()

    stub(fn conn ->
      {:ok, body, conn} = Plug.Conn.read_body(conn)

      send(parent, {
        :req,
        conn.method,
        conn.request_path,
        body,
        get_req(conn, "authorization"),
        get_req(conn, "content-length")
      })

      Plug.Conn.send_resp(conn, 200, "")
    end)

    assert {:ok, meta} =
             S3.store({:binary, "hello"}, "Photo.PNG", id: "abc123", mime_type: "image/png")

    assert meta.path == "lazypock/abc123/original.png"
    assert meta.size == 5
    assert meta.mime_type == "image/png"

    # The streamed body must carry an explicit content-length: R2 answers 411
    # when the PUT is chunked.
    assert_receive {:req, "PUT", "/bucket/lazypock/abc123/original.png", "hello", [auth], ["5"]}

    assert auth =~ "AWS4-HMAC-SHA256"

    assert auth =~
             "SignedHeaders=content-length;content-type;host;x-amz-content-sha256;x-amz-date"
  end

  test "get returns the body and maps 404 to :not_found" do
    stub(fn conn ->
      if conn.request_path =~ "missing" do
        Plug.Conn.send_resp(conn, 404, "NoSuchKey")
      else
        Plug.Conn.send_resp(conn, 200, "object-bytes")
      end
    end)

    assert {:ok, "object-bytes"} = S3.get(%{"storage_path" => "lazypock/a/original.png"})
    assert {:error, :not_found} = S3.get(%{"storage_path" => "lazypock/missing.png"})
  end

  test "url uses the public base URL, falling back to the app route" do
    assert S3.url(%{"id" => "abc", "storage_path" => "lazypock/abc/original.png"}) ==
             "https://cdn.test/lazypock/abc/original.png"

    Settings.put(
      Map.put(Settings.get(), "storage", %{
        "backend" => "s3",
        "endpoint" => "https://s3.test",
        "bucket" => "bucket"
      })
    )

    Storage.clear_cache()

    assert S3.url(%{"id" => "abc", "storage_path" => "lazypock/abc/original.png"}) ==
             "/api/files/abc"
  end

  test "delete removes the object and every object under its prefix" do
    parent = self()

    stub(fn conn ->
      case conn.method do
        "GET" ->
          body =
            "<ListBucketResult>" <>
              "<Contents><Key>lazypock/abc/thumb.webp</Key></Contents>" <>
              "<Contents><Key>lazypock/abc/original.png</Key></Contents>" <>
              "</ListBucketResult>"

          Plug.Conn.send_resp(conn, 200, body)

        "DELETE" ->
          send(parent, {:deleted, conn.request_path})
          Plug.Conn.send_resp(conn, 204, "")
      end
    end)

    assert :ok = S3.delete(%{"id" => "abc", "storage_path" => "lazypock/abc/original.png"})
    assert_receive {:deleted, "/bucket/lazypock/abc/original.png"}
    assert_receive {:deleted, "/bucket/lazypock/abc/thumb.webp"}
  end

  test "scale renders and uploads a preset variant on a cache miss" do
    png = Lazypock.TestImage.tiny_png!(120, 80)

    stub(fn conn ->
      cond do
        conn.method == "GET" and String.ends_with?(conn.request_path, "/thumb.webp") ->
          Plug.Conn.send_resp(conn, 404, "")

        conn.method == "GET" ->
          Plug.Conn.send_resp(conn, 200, png)

        conn.method == "PUT" ->
          {:ok, body, conn} = Plug.Conn.read_body(conn)
          send(self(), {:variant, conn.request_path, body})
          Plug.Conn.send_resp(conn, 200, "")
      end
    end)

    preset = %{
      "name" => "thumb",
      "width" => 50,
      "height" => 50,
      "fit" => "cover",
      "quality" => 80
    }

    assert {:ok, binary, "image/webp"} =
             S3.scale(%{"id" => "abc", "storage_path" => "lazypock/abc/original.png"}, preset)

    assert binary_part(binary, 0, 4) == "RIFF"

    receive do
      {:variant, path, body} ->
        assert path == "/bucket/lazypock/abc/thumb.webp"
        assert binary_part(body, 0, 4) == "RIFF"
    after
      1_000 -> flunk("expected the variant to be uploaded")
    end
  end

  test "test_connection runs PUT, HEAD, GET and DELETE" do
    stub(fn conn ->
      case conn.method do
        "PUT" -> Plug.Conn.send_resp(conn, 200, "")
        "HEAD" -> Plug.Conn.send_resp(conn, 200, "")
        "GET" -> Plug.Conn.send_resp(conn, 200, "lazypock")
        "DELETE" -> Plug.Conn.send_resp(conn, 204, "")
      end
    end)

    steps = S3.test_connection()
    assert Enum.all?(steps, & &1.ok)
    assert Enum.map(steps, & &1.step) == [:put, :head, :get, :delete]
  end

  test "unconfigured storage fails with :not_configured" do
    Settings.put(Map.put(Settings.get(), "storage", %{"backend" => "s3"}))
    Storage.clear_cache()

    assert {:error, :not_configured} = S3.get(%{"storage_path" => "k"})
    assert {:error, :not_configured} = S3.store({:binary, "x"}, "a.txt", [])
  end

  defp get_req(conn, name) do
    Plug.Conn.get_req_header(conn, name)
  end
end
