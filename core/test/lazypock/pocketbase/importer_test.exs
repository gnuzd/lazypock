defmodule Lazypock.PocketBase.ImporterTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.PocketBase.Importer
  alias Lazypock.Repo
  alias Lazypock.Collections.Registry


  @moduledoc """
  End-to-end import from a synthetic PocketBase SQLite database (built with the
  sqlite3 CLI) into LazyPock: collections, records, relations, auth users,
  external auths, and files.
  """

  setup do
    dir =
      Path.join(System.tmp_dir!(), "lazypock-pb-test-#{System.unique_integer([:positive])}")

    # unique_integer restarts per VM — clean any leftover from a previous run
    File.rm_rf!(dir)
    File.mkdir_p!(Path.join([dir, "storage", "pb_articles", "rec_abc123"]))
    File.mkdir_p!(Path.join([dir, "storage", "pb_users", "rec_xyz789"]))

    db = Path.join(dir, "data.db")
    File.write!(Path.join([dir, "storage", "pb_articles", "rec_abc123", "cover.png"]), "PNG-BYTES")

    # PocketBase-style schema: _collections + collection tables
    sql = """
    CREATE TABLE _collections (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL,
      type TEXT NOT NULL DEFAULT 'base',
      system BOOLEAN NOT NULL DEFAULT false,
      schema TEXT NOT NULL DEFAULT '[]',
      indexes TEXT NOT NULL DEFAULT '[]',
      listRule TEXT, viewRule TEXT, createRule TEXT, updateRule TEXT, deleteRule TEXT, manageRule TEXT,
      options TEXT NOT NULL DEFAULT '{}',
      created TEXT NOT NULL, updated TEXT NOT NULL
    );

    INSERT INTO _collections (id, name, type, system, schema, indexes, listRule, viewRule, createRule, updateRule, deleteRule, manageRule, options, created, updated) VALUES
      ('coll_base_01', 'pb_articles', 'base', 0,
       '[{"name":"title","type":"text","required":true,"system":false,"options":{}},
         {"name":"views","type":"number","required":false,"system":false,"options":{}},
         {"name":"cover","type":"file","required":false,"system":false,"options":{"maxSelect":1,"mimeTypes":["image/png"]}},
         {"name":"author","type":"relation","required":false,"system":false,"options":{"collectionId":"coll_auth_02","maxSelect":1}}]',
       '[]', '', NULL, NULL, NULL, NULL, NULL, '{}', '2024-01-01 00:00:00.000Z', '2024-01-01 00:00:00.000Z'),
      ('coll_auth_02', 'pb_users', 'auth', 0,
       '[{"name":"email","type":"email","required":true,"system":true,"options":{}},
         {"name":"passwordHash","type":"password","required":true,"system":true,"options":{}},
         {"name":"name","type":"text","required":false,"system":false,"options":{}}]',
       '[]', '', NULL, NULL, 'id = @request.auth.id', NULL, NULL, '{}', '2024-01-01 00:00:00.000Z', '2024-01-01 00:00:00.000Z'),
      ('sys_coll_03', '_externalAuths', 'base', 1, '[]', '[]', NULL, NULL, NULL, NULL, NULL, NULL, '{}', '2024-01-01 00:00:00.000Z', '2024-01-01 00:00:00.000Z');

    CREATE TABLE pb_articles (
      id TEXT PRIMARY KEY, created TEXT NOT NULL, updated TEXT NOT NULL,
      title TEXT NOT NULL, views INTEGER, cover TEXT, author TEXT
    );
    INSERT INTO pb_articles (id, created, updated, title, views, cover, author) VALUES
      ('rec_abc123', '2024-01-02 10:00:00.000Z', '2024-01-03 11:00:00.000Z', 'Hello PB', 42, 'cover.png', 'rec_xyz789'),
      ('rec_def456', '2024-01-04 10:00:00.000Z', '2024-01-04 10:00:00.000Z', 'Second', 7, NULL, NULL);

    CREATE TABLE pb_users (
      id TEXT PRIMARY KEY, created TEXT NOT NULL, updated TEXT NOT NULL,
      email TEXT NOT NULL, passwordHash TEXT NOT NULL, verified INTEGER DEFAULT 0,
      tokenKey TEXT, emailVisibility INTEGER DEFAULT 1, name TEXT
    );
    INSERT INTO pb_users (id, created, updated, email, passwordHash, verified, tokenKey, emailVisibility, name) VALUES
      ('rec_xyz789', '2024-01-01 09:00:00.000Z', '2024-01-01 09:00:00.000Z',
       'author@test.com', '$2b$10$fakebcrypthash', 1, 'tokkey', 1, 'The Author');

    CREATE TABLE _externalAuths (
      id TEXT PRIMARY KEY, collectionRef TEXT NOT NULL, recordRef TEXT NOT NULL,
      provider TEXT NOT NULL, providerId TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL
    );
    INSERT INTO _externalAuths (id, collectionRef, recordRef, provider, providerId, created, updated) VALUES
      ('ext_0001', 'coll_auth_02', 'rec_xyz789', 'google', 'google-user-1', '2024-01-01 09:00:00.000Z', '2024-01-01 09:00:00.000Z');
    """

    {_, 0} = System.cmd("sqlite3", [db, sql], stderr_to_stdout: true)

    %{dir: dir, db: db}
  end

  defp article_table_exists? do
    Ecto.Adapters.SQL.query!(Repo, "SELECT to_regclass('pb_articles')", [])
    |> Map.fetch!(:rows)
    |> hd()
    |> hd() != nil
  end

  describe "import_all/1" do
    test "dry run reports without importing", %{db: db, dir: dir} do
      summary = Importer.import_all(pb_db: db, storage_dir: Path.join(dir, "storage"), dry_run: true)

      assert summary.dry_run == true
      assert summary.collections == 2
      assert "pb_articles" in summary.collections_list
      assert "pb_users" in summary.collections_list
      refute article_table_exists?()
    end

    test "imports collections, records, relations, auth, files and external auths", %{
      db: db,
      dir: dir
    } do
      summary =
        Importer.import_all(
          pb_db: db,
          storage_dir: Path.join(dir, "storage"),
          yes: true,
          id_map_file: nil
        )

      assert summary.dry_run == false
      assert summary.collections == 2
      assert summary.records == 3
      assert summary.files == 1
      assert summary.external_auths == 1
      assert summary.warnings == []

      Registry.reload!()

      # Collection created with mapped fields + rules
      {:ok, articles} = Registry.get("pb_articles")
      assert articles.type == "base"
      assert articles.rules["listRule"] == ""
      assert articles.rules["createRule"] == nil
      names = Enum.map(articles.fields, & &1.name)
      assert "title" in names
      assert "views" in names
      assert "cover" in names
      assert "author" in names
      cover = Enum.find(articles.fields, &(&1.name == "cover"))
      assert cover.type == "file"

      {:ok, users} = Registry.get("pb_users")
      assert users.type == "auth"
      assert users.rules["updateRule"] == "id = @request.auth.id"

      # Records imported with rewritten ids + preserved timestamps
      {:ok, %{rows: [[id, created, _title, author]]}} =
        Ecto.Adapters.SQL.query(
          Repo,
          "SELECT id, created_at, title, author FROM pb_articles WHERE title = 'Hello PB'",
          []
        )

      assert Ecto.UUID.cast!(id) == Importer.uuid5("pb_articles", "rec_abc123")
      assert DateTime.truncate(created, :second) == ~U[2024-01-02 10:00:00Z]
      # Relation rewritten: old author id rec_xyz789 → uuid of users/rec_xyz789
      assert Ecto.UUID.cast!(author) == Importer.uuid5("pb_users", "rec_xyz789")

      # File field rewritten from filename to a LazyPock file id
      {:ok, %{rows: [[cover_id]]}} =
        Ecto.Adapters.SQL.query(
          Repo,
          "SELECT cover FROM pb_articles WHERE title = 'Hello PB'",
          []
        )

      assert is_binary(cover_id) and cover_id != "cover.png"
      assert {:ok, file} = Lazypock.Files.Store.get(cover_id)
      assert file["filename"] == "cover.png"
      {:ok, binary} = Lazypock.Files.Store.read(file)
      assert binary == "PNG-BYTES"

      # Auth user imported with bcrypt hash + verified
      {:ok, %{rows: [[email, pw_hash, verified]]}} =
        Ecto.Adapters.SQL.query(
          Repo,
          "SELECT email, password_hash, verified FROM pb_users WHERE id = $1",
          [Ecto.UUID.dump!(Importer.uuid5("pb_users", "rec_xyz789"))]
        )

      assert email == "author@test.com"
      assert pw_hash == "$2b$10$fakebcrypthash"
      assert verified == true

      # External auth linked with rewritten user id
      {:ok, %{rows: [[provider, provider_id, user_id]]}} =
        Ecto.Adapters.SQL.query(
          Repo,
          "SELECT provider, provider_id, user_id FROM _external_auths",
          []
        )

      assert provider == "google"
      assert provider_id == "google-user-1"
      assert Ecto.UUID.cast!(user_id) == Importer.uuid5("pb_users", "rec_xyz789")

      # Cleanup
      Lazypock.Files.Store.delete(cover_id)
      cleanup_ddl()
    end
  end

  defp cleanup_ddl do
    Lazypock.Schema.DDL.drop_collection("pb_articles")
    Lazypock.Schema.DDL.drop_collection("pb_users")
  end
end
