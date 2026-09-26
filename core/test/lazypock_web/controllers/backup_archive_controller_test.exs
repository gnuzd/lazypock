defmodule LazypockWeb.BackupArchiveControllerTest do
  @moduledoc """
  HTTP surface for large-DB backup/restore:

    * `GET /api/export` keeps its published JSON shape (frozen contract);
    * `GET /api/export/archive` streams an NDJSON zip;
    * `GET /api/import/preflight` lets a UI warn before uploading;
    * `POST /api/import` accepts JSON or a multipart archive and requires an
      explicit confirmation above the undo threshold.
  """
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token
  alias Lazypock.Collections.Registry
  alias Lazypock.Repo
  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord

  @password "password"

  defp random_name(prefix) do
    suffix = :crypto.strong_rand_bytes(4) |> Base.encode16() |> String.downcase()
    prefix <> suffix
  end

  defp auth_conn(conn) do
    email = "admin_#{:erlang.unique_integer([:positive])}@test.com"

    superuser = %SuperUser{
      id: Ecto.UUID.generate(),
      email: email,
      password_hash: Bcrypt.hash_pwd_salt(@password)
    }

    Repo.insert!(superuser)
    {:ok, token} = Token.generate_access_token(superuser)
    put_req_header(conn, "authorization", "Bearer #{token}")
  end

  defp json_post(conn, path, payload) do
    conn
    |> put_req_header("content-type", "application/json")
    |> post(path, Jason.encode!(Map.put_new(payload, "password", @password)))
  end

  # Hand-built multipart body — Plug has no public builder, and this is the one
  # transport that has to work for a multi-GB archive.
  defp multipart_post(conn, path, filename, content, fields) do
    boundary = "----lazypocktest#{System.unique_integer([:positive])}"

    parts =
      Enum.map(fields, fn {k, v} ->
        ["--#{boundary}\r\n", "Content-Disposition: form-data; name=\"#{k}\"\r\n\r\n", v, "\r\n"]
      end) ++
        [
          "--#{boundary}\r\n",
          "Content-Disposition: form-data; name=\"file\"; filename=\"#{filename}\"\r\n",
          "Content-Type: application/zip\r\n\r\n",
          content,
          "\r\n--#{boundary}--\r\n"
        ]

    conn
    |> put_req_header("content-type", "multipart/form-data; boundary=#{boundary}")
    |> post(path, IO.iodata_to_binary(parts))
  end

  setup do
    Ecto.Adapters.SQL.query!(Repo, "DELETE FROM _import_snapshots", [])

    on_exit(fn ->
      System.delete_env("LAZYPOCK_IMPORT_UNDO_MAX_MB")
      System.delete_env("LAZYPOCK_BACKUP_DIR")
    end)

    :ok
  end

  describe "GET /api/export (frozen contract)" do
    test "still returns the canonical JSON envelope, not the archive", %{conn: conn} do
      name = random_name("ctr_")
      {:ok, _} = DDL.create_collection(name, fields: [%{"name" => "title", "type" => "text"}])
      {:ok, _} = GenericRecord.insert(name, %{"title" => "json-still-works"})

      body =
        conn
        |> auth_conn()
        |> get("/api/export")
        |> json_response(200)

      assert %{"collections" => collections} = body
      assert Enum.any?(collections, &(&1["name"] == name))

      # The published schema requires records inline for this response.
      assert Enum.any?(collections, &is_list(&1["records"]))
    end
  end

  describe "GET /api/export/archive" do
    test "returns a downloadable NDJSON zip", %{conn: conn} do
      name = random_name("ctr_")
      {:ok, _} = DDL.create_collection(name, fields: [%{"name" => "title", "type" => "text"}])
      {:ok, _} = GenericRecord.insert(name, %{"title" => "in-archive"})

      conn =
        conn
        |> auth_conn()
        |> get("/api/export/archive")

      assert conn.status == 200
      assert get_resp_header(conn, "content-type") |> hd() =~ "application/zip"
      assert get_resp_header(conn, "content-disposition") |> hd() =~ "attachment"

      assert <<"PK", _::binary>> = conn.resp_body

      dir = Path.join(System.tmp_dir!(), "lz-ctrl-#{System.unique_integer([:positive])}")
      File.mkdir_p!(dir)
      zip = Path.join(dir, "out.zip")
      File.write!(zip, conn.resp_body)

      {:ok, _} = :zip.extract(String.to_charlist(zip), [{:cwd, String.to_charlist(dir)}])
      assert File.exists?(Path.join(dir, "manifest.json"))

      ndjson = File.read!(Path.join([dir, "data", "#{name}.ndjson"]))
      assert Jason.decode!(String.trim(ndjson))["title"] == "in-archive"

      File.rm_rf(dir)
    end

    test "requires a superuser", %{conn: conn} do
      assert conn |> get("/api/export/archive") |> response(403)
    end
  end

  describe "GET /api/import/preflight" do
    test "reports size, threshold and undo availability", %{conn: conn} do
      body =
        conn
        |> auth_conn()
        |> get("/api/import/preflight")
        |> json_response(200)

      assert Map.has_key?(body, "db_size_bytes")
      assert Map.has_key?(body, "threshold_mb")
      assert is_boolean(body["undo_available"])
      assert is_boolean(body["neon_hosted"])
    end

    test "requires a superuser", %{conn: conn} do
      assert conn |> get("/api/import/preflight") |> response(403)
    end

    test "import/status also carries the preflight block", %{conn: conn} do
      body =
        conn
        |> auth_conn()
        |> get("/api/import/status")
        |> json_response(200)

      assert Map.has_key?(body, "snapshot")
      assert Map.has_key?(body, "preflight")
    end
  end

  describe "POST /api/import confirmation gate" do
    test "refuses a large import without confirmation, then accepts it with", %{conn: conn} do
      # 0 MB threshold makes "above the threshold" deterministic.
      System.put_env("LAZYPOCK_IMPORT_UNDO_MAX_MB", "0")

      name = random_name("ctr_gate_")
      {:ok, _} = DDL.create_collection(name, fields: [%{"name" => "title", "type" => "text"}])
      {:ok, _} = GenericRecord.insert(name, %{"title" => "seed"})
      Registry.reload!()

      payload = %{
        "collections" => [
          %{
            "name" => name,
            "type" => "base",
            "schema" => [],
            "records" => [%{"title" => "imported"}]
          }
        ]
      }

      conn = auth_conn(conn)

      refused = json_post(conn, "/api/import", payload)
      assert refused.status == 409
      body = json_response(refused, 409)
      assert body["requires_confirmation"] == true
      assert body["reason"] == "large_import"
      assert body["message"] =~ "no one-click rollback"
      assert body["preflight"]["undo_available"] == false

      # Nothing was written.
      assert GenericRecord.all(name) |> Enum.map(& &1["title"]) == ["seed"]

      accepted = json_post(conn, "/api/import", Map.put(payload, "confirm", true))
      assert json_response(accepted, 200)["errors"] == []

      assert GenericRecord.all(name) |> Enum.map(& &1["title"]) |> Enum.sort() ==
               ["imported", "seed"]
    end

    test "neon notice is included when detected", %{conn: conn} do
      System.put_env("LAZYPOCK_IMPORT_UNDO_MAX_MB", "0")
      System.put_env("LAZYPOCK_NEON_HOSTED", "1")

      on_exit(fn -> System.delete_env("LAZYPOCK_NEON_HOSTED") end)

      name = random_name("ctr_neon_")
      {:ok, _} = DDL.create_collection(name, fields: [])
      Registry.reload!()

      body =
        conn
        |> auth_conn()
        |> json_post("/api/import", %{
          "collections" => [%{"name" => name, "type" => "base", "schema" => []}]
        })
        |> json_response(409)

      assert body["message"] =~ "Neon"
      assert body["preflight"]["neon_hosted"] == true
    end
  end

  describe "POST /api/import with an archive upload" do
    test "restores records from an uploaded archive", %{conn: conn} do
      name = random_name("ctr_zip_")
      {:ok, _} = DDL.create_collection(name, fields: [%{"name" => "title", "type" => "text"}])
      {:ok, inserted} = GenericRecord.insert(name, %{"title" => "from-archive"})
      Registry.reload!()

      # Export, wipe, then restore through the HTTP multipart path.
      export_conn =
        conn
        |> auth_conn()
        |> get("/api/export/archive")

      assert export_conn.status == 200

      Ecto.Adapters.SQL.query!(Repo, "DELETE FROM \"#{name}\"", [])
      assert GenericRecord.all(name) == []

      restore_conn =
        build_conn()
        |> auth_conn()
        |> multipart_post("/api/import", "backup.zip", export_conn.resp_body, %{
          "password" => @password,
          "deleteMissing" => "false",
          "atomic" => "batch"
        })

      body = json_response(restore_conn, 200)
      assert body["errors"] == []
      assert Enum.any?(body["imported"], &(&1["name"] == name))

      records = GenericRecord.all(name)
      assert length(records) == 1
      assert hd(records)["id"] == inserted["id"]
      assert hd(records)["title"] == "from-archive"
    end
  end
end
