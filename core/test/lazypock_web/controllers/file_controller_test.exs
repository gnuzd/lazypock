defmodule LazypockWeb.FileControllerTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token
  alias Lazypock.Repo

  @moduledoc """
  File upload / download / delete endpoints. Requires superuser auth.
  """

  defp superuser_token do
    su = %SuperUser{
      id: Ecto.UUID.generate(),
      email: "admin#{System.unique_integer([:positive])}@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(su)
    {:ok, token} = Token.generate_access_token(su)
    token
  end

  defp auth_conn(conn), do: put_req_header(conn, "authorization", "Bearer #{superuser_token()}")

  defp upload_body(binary, filename) do
    path = Path.join(System.tmp_dir!(), "lazypock-fc-#{System.unique_integer([:positive])}#{Path.extname(filename)}")
    File.write!(path, binary)

    %Plug.Upload{
      path: path,
      filename: filename,
      content_type: "text/plain"
    }
  end

  describe "POST /api/files" do
    test "uploads a file and returns its metadata" do
      conn =
        auth_conn(build_conn())
        |> put_req_header("content-type", "multipart/form-data")
        |> post("/api/files", %{"file" => upload_body("hello world", "notes.txt")})

      body = json_response(conn, 201)
      assert body["filename"] == "notes.txt"
      assert body["mimeType"] == "text/plain"
      assert body["size"] == 11
      assert body["id"] != nil

      # Cleanup
      auth_conn(build_conn()) |> delete("/api/files/#{body["id"]}") |> response(204)
    end

    test "rejects anonymous uploads" do
      conn = post(build_conn(), "/api/files", %{"file" => upload_body("x", "a.txt")})
      assert response(conn, 403)
    end

    test "allows uploads with an auth-collection user token" do
      name = "files_users_#{System.unique_integer([:positive])}"

      {:ok, _} =
        Lazypock.Schema.DDL.create_collection(name,
          type: "auth",
          fields: [
            %{"name" => "email", "type" => "email", "required" => true},
            %{"name" => "password_hash", "type" => "password", "required" => true}
          ]
        )

      Lazypock.Collections.Registry.reload!()
      {:ok, user} = Lazypock.Schemas.GenericRecord.insert(name, %{"email" => "app@test.com", "password_hash" => "x"})
      {:ok, token} = Token.generate_user_token(user, name)

      conn =
        build_conn()
        |> put_req_header("authorization", "Bearer #{token}")
        |> put_req_header("content-type", "multipart/form-data")
        |> post("/api/files", %{"file" => upload_body("app upload", "app.txt")})

      body = json_response(conn, 201)
      assert body["filename"] == "app.txt"
      Lazypock.Files.Store.delete(body["id"])
    end

    test "rejects uploads missing the file part" do
      conn = auth_conn(build_conn()) |> post("/api/files", %{})
      assert response(conn, 400)
    end
  end

  describe "GET /api/files" do
    test "lists uploaded files for superusers" do
      {:ok, file} = Lazypock.Files.Store.store("hello", "listed.txt", [])
      conn = auth_conn(build_conn()) |> get("/api/files")
      body = json_response(conn, 200)
      assert body["total"] >= 1
      assert Enum.any?(body["items"], &(&1["id"] == file["id"]))

      Lazypock.Files.Store.delete(file["id"])
    end

    test "rejects anonymous listing" do
      conn = get(build_conn(), "/api/files")
      assert response(conn, 403)
    end
  end

  describe "GET /api/files/:id" do
    test "returns the file binary with its mime type" do
      {:ok, file} = Lazypock.Files.Store.store("file-body-here", "doc.txt", [])

      conn = auth_conn(build_conn()) |> get("/api/files/#{file["id"]}")
      assert response(conn, 200) == "file-body-here"
      assert get_resp_header(conn, "content-type") == ["text/plain"]

      Lazypock.Files.Store.delete(file["id"])
    end

    test "returns 404 for an unknown file" do
      conn = auth_conn(build_conn()) |> get("/api/files/#{Ecto.UUID.generate()}")
      assert json_response(conn, 404)["message"] == "File not found"
    end
  end

  describe "DELETE /api/files/:id" do
    test "deletes a file" do
      {:ok, file} = Lazypock.Files.Store.store("to-delete", "del.txt", [])
      conn = auth_conn(build_conn()) |> delete("/api/files/#{file["id"]}")
      assert response(conn, 204)
      assert {:error, :not_found} = Lazypock.Files.Store.get(file["id"])
    end

    test "rejects anonymous delete" do
      {:ok, file} = Lazypock.Files.Store.store("to-delete", "del.txt", [])
      conn = delete(build_conn(), "/api/files/#{file["id"]}")
      assert response(conn, 403)
      Lazypock.Files.Store.delete(file["id"])
    end
  end

  describe "GET /api/files/:id/thumbs/:size" do
    test "serves a generated thumbnail when present" do
      path =
        Path.join(System.tmp_dir!(), "lazypock-fc-thumb-#{System.unique_integer([:positive])}.png")

      System.cmd("magick", ["-size", "100x80", "gradient:red-blue", path])
      binary = File.read!(path)
      File.rm(path)

      {:ok, file} =
        Lazypock.Files.Store.store(binary, "img.png", thumb_sizes: ["100x100"])

      if file["thumbs"] != %{} do
        conn = auth_conn(build_conn()) |> get("/api/files/#{file["id"]}/thumbs/100x100")
        assert response(conn, 200) != ""
        assert List.first(get_resp_header(conn, "content-type")) == "image/webp"
      end

      Lazypock.Files.Store.delete(file["id"])
    end

    test "returns 404 when no thumbnail exists for the size" do
      {:ok, file} = Lazypock.Files.Store.store("plain", "notes.txt", [])

      conn = auth_conn(build_conn()) |> get("/api/files/#{file["id"]}/thumbs/100x100")
      assert response(conn, 404)

      Lazypock.Files.Store.delete(file["id"])
    end
  end
end
