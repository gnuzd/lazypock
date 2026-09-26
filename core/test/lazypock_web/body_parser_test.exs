defmodule LazypockWeb.BodyParserTest do
  @moduledoc """
  The 8 MB `Plug.Parsers` default is the transport-level wall that made large
  backup imports impossible. The raise must apply to the archive-import route
  only.
  """
  use ExUnit.Case, async: false

  alias LazypockWeb.BodyParser

  defp parse(conn) do
    conn
    |> Plug.Conn.put_req_header("content-type", "application/json")
    |> then(&BodyParser.call(&1, BodyParser.init([])))
  end

  defp json_conn(path, bytes) do
    body = Jason.encode!(%{"pad" => String.duplicate("x", bytes)})
    Plug.Test.conn(:post, path, body)
  end

  test "a body over the 8 MB default is parsed on the import route" do
    conn = parse(json_conn("/api/import", 9_000_000))

    assert byte_size(conn.body_params["pad"]) == 9_000_000
  end

  test "the same body is still rejected everywhere else" do
    assert_raise Plug.Parsers.RequestTooLargeError, fn ->
      parse(json_conn("/api/export", 9_000_000))
    end

    assert_raise Plug.Parsers.RequestTooLargeError, fn ->
      parse(json_conn("/api/collections", 9_000_000))
    end
  end

  test "a body under the default still parses on any route" do
    conn = parse(json_conn("/api/collections", 1_000))
    assert byte_size(conn.body_params["pad"]) == 1_000
  end

  test "multipart parts over 8 MB become a Plug.Upload on the import route" do
    boundary = "----lazypocktest"
    payload = String.duplicate("z", 9_000_000)

    body =
      IO.iodata_to_binary([
        "--#{boundary}\r\n",
        "Content-Disposition: form-data; name=\"password\"\r\n\r\nsecret\r\n",
        "--#{boundary}\r\n",
        "Content-Disposition: form-data; name=\"file\"; filename=\"backup.zip\"\r\n",
        "Content-Type: application/zip\r\n\r\n",
        payload,
        "\r\n--#{boundary}--\r\n"
      ])

    conn =
      Plug.Test.conn(:post, "/api/import", body)
      |> Plug.Conn.put_req_header("content-type", "multipart/form-data; boundary=#{boundary}")

    parsed = BodyParser.call(conn, BodyParser.init([]))

    assert %Plug.Upload{filename: "backup.zip"} = parsed.body_params["file"]
    # Streamed to a temp file rather than held in memory.
    assert File.stat!(parsed.body_params["file"].path).size == 9_000_000
    assert parsed.body_params["password"] == "secret"

    File.rm(parsed.body_params["file"].path)
  end

  test "the limit is configurable and does not break the default path" do
    System.put_env("LAZYPOCK_IMPORT_MAX_MB", "1")
    # A fresh persistent_term key is used per length, so the new limit applies.
    assert_raise Plug.Parsers.RequestTooLargeError, fn ->
      parse(json_conn("/api/import", 2_000_000))
    end

    System.delete_env("LAZYPOCK_IMPORT_MAX_MB")
  end
end
