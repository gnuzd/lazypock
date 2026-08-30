defmodule Lazypock.Schema.Views do
  @moduledoc """
  PocketBase-parity helpers for "view" collections — read-only collections
  whose data is populated from a plain SQL `SELECT` query over other
  collections.

  Mirrors PocketBase's `core/view.go` semantics:

    * the query must be a single SELECT statement (`;` is rejected),
    * wildcard (`*`) columns are rejected (avoids accidentally leaking
      hidden columns such as password hashes),
    * the query must expose a unique `id` column — when the column isn't
      text-typed it is `CAST` to TEXT so record lookups
      (`WHERE id = $1`) behave like on regular collections,
    * fields are auto-generated from the query result columns,
    * the physical DB object is created as
      `CREATE VIEW name AS SELECT ... FROM (<query>)` (the query is wrapped
      in a secondary SELECT, exactly like PocketBase).

  All functions run raw SQL against the current Repo connection, so they can
  be called inside the DDL engine's transactions.
  """

  alias Lazypock.Repo
  alias Lazypock.Schema.TypeMapper

  # Sample size used by the dry-run endpoint (PocketBase uses 10).
  @sample_size 10

  # Postgres text-family types — an `id` column of any of these needs no cast.
  @text_types MapSet.new(["text", "character varying", "character", "char", "citext"])

  @doc """
  Builds the field definitions for a view query by introspecting its result
  columns (via a temporary view).

  Returns `{:ok, [field_def]}` or `{:error, message}`.
  """
  @spec build_fields(String.t()) :: {:ok, [map()]} | {:error, String.t()}
  def build_fields(query) when is_binary(query) do
    with {:ok, normalized} <- normalize_query(query),
         {:ok, columns} <- introspect(normalized),
         :ok <- ensure_id_column(columns) do
      {:ok, build_field_defs(columns)}
    end
  end

  @doc """
  Creates (or replaces) the physical Postgres view for a view collection.

  The query is validated first via a temporary-view introspection, and only
  then is the existing view dropped (CASCADE) and recreated — a failing query
  never destroys a working view. Returns `{:ok, columns}` where `columns` is
  the introspected `[{column_name, pg_type}]` list.
  """
  @spec create_view(String.t(), String.t()) ::
          {:ok, [{String.t(), String.t()}]} | {:error, term()}
  def create_view(name, query) when is_binary(name) and is_binary(query) do
    with {:ok, normalized} <- normalize_query(query),
         {:ok, columns} <- introspect(normalized),
         :ok <- ensure_id_column(columns) do
      Ecto.Adapters.SQL.query!(
        Repo,
        "DROP VIEW IF EXISTS #{TypeMapper.quote_ident(name)} CASCADE",
        []
      )

      Ecto.Adapters.SQL.query!(Repo, build_create_view_sql(name, normalized, columns), [])
      {:ok, columns}
    end
  end

  @doc """
  Drops the physical view for a collection (CASCADE, so dependent views are
  dropped too — they are recreated by `Lazypock.Schema.DDL.sync_dependent_views/1`).
  """
  @spec drop_view(String.t()) :: :ok
  def drop_view(name) when is_binary(name) do
    Ecto.Adapters.SQL.query!(
      Repo,
      "DROP VIEW IF EXISTS #{TypeMapper.quote_ident(name)} CASCADE",
      []
    )

    :ok
  end

  @doc """
  PocketBase `dryRunView` parity: validates the query and returns the
  generated `fields` plus a `sample` of up to `sample_size` records.

  The sample ids are checked for emptiness and uniqueness (PocketBase
  behavior). Returns `{:ok, %{fields: ..., sample: ...}}` or `{:error, msg}`.
  """
  @spec dry_run(String.t(), pos_integer()) ::
          {:ok, %{fields: [map()], sample: [map()]}} | {:error, String.t()}
  def dry_run(query, sample_size \\ @sample_size) when is_binary(query) do
    with {:ok, normalized} <- normalize_query(query),
         {:ok, columns} <- introspect(normalized),
         :ok <- ensure_id_column(columns) do
      fields = build_field_defs(columns)
      sql = "SELECT #{select_list_sql(columns)} FROM (#{normalized}) AS _v LIMIT #{sample_size}"

      case sample_rows(sql) do
        {:ok, rows} ->
          ids = Enum.map(rows, & &1["id"])

          cond do
            Enum.any?(ids, &(is_nil(&1) or &1 == "")) ->
              {:error, "the query could return records with empty or invalid ids"}

            length(ids) != MapSet.size(MapSet.new(ids)) ->
              {:error, "the query could return records with non-unique ids"}

            true ->
              {:ok, %{fields: fields, sample: rows}}
          end

        {:error, msg} ->
          {:error, msg}
      end
    end
  end

  # Runs the sample query and returns the rows as maps keyed by column name.
  defp sample_rows(sql) do
    case Ecto.Adapters.SQL.query(Repo, sql, []) do
      {:ok, %{columns: cols, rows: rows}} ->
        {:ok, Enum.map(rows, fn row -> cols |> Enum.zip(row) |> Map.new() end)}

      {:error, %Postgrex.Error{} = err} ->
        {:error, Exception.message(err)}
    end
  end

  # ── Private helpers ────────────────────────────────

  # Normalizes the query text (trim whitespace + leading/trailing semicolons)
  # and rejects multiple inline statements. The `;` check ignores semicolons
  # inside quoted string literals and comments.
  defp normalize_query(query) when is_binary(query) do
    trimmed = query |> String.trim() |> String.trim(";")

    cond do
      trimmed == "" ->
        {:error, "view query is required"}

      true ->
        scrubbed = scrub(query)

        if String.contains?(scrubbed, ";") do
          {:error, "multiple statements are not supported"}
        else
          case check_wildcards(scrubbed) do
            :ok -> {:ok, trimmed}
            {:error, _} = err -> err
          end
        end
    end
  end

  # Rejects wildcard columns (bare `*` and `table.*`) in the SELECT list —
  # PocketBase rejects them to avoid accidentally leaking sensitive columns
  # (e.g. password hashes). `count(*)` / `sum(*)` style function calls are
  # allowed (the star is inside parentheses).
  defp check_wildcards(scrubbed) do
    cond do
      Regex.match?(~r/\bselect\s+(distinct\s+)?\*/i, scrubbed) ->
        {:error,
         "wildcard columns (*) are not supported - manually type the collection field names you want the view query to have"}

      Regex.match?(~r/\.\*\s*(\bas\b|,|\bfrom\b)/i, scrubbed) ->
        {:error,
         "wildcard columns (*) are not supported - manually type the collection field names you want the view query to have"}

      true ->
        :ok
    end
  end

  # Introspects the result columns of a query by materializing it as a
  # temporary view (mirrors PocketBase's getQueryTableInfo). Errors are
  # returned as {:error, postgres_message} — a non-SELECT query, a missing
  # table/column, etc. all fail here.
  defp introspect(query) do
    probe = "_lazypock_probe_" <> Base.encode16(:crypto.strong_rand_bytes(4), case: :lower)

    try do
      Repo.transaction(fn ->
        Ecto.Adapters.SQL.query!(
          Repo,
          "CREATE TEMP VIEW #{TypeMapper.quote_ident(probe)} AS SELECT * FROM (#{query}) AS _v",
          []
        )

        info =
          Ecto.Adapters.SQL.query!(
            Repo,
            """
            SELECT column_name, data_type
            FROM information_schema.columns
            WHERE table_name = $1
            ORDER BY ordinal_position
            """,
            [probe]
          )

        Ecto.Adapters.SQL.query!(
          Repo,
          "DROP VIEW IF EXISTS #{TypeMapper.quote_ident(probe)}",
          []
        )

        Enum.map(info.rows, fn [name, type] -> {name, type} end)
      end)
    rescue
      e in Postgrex.Error -> {:error, Exception.message(e)}
    end
  end

  defp ensure_id_column(columns) do
    case Enum.find(columns, fn {name, _type} -> String.downcase(name) == "id" end) do
      nil ->
        {:error,
         "missing required id column (you can use (ROW_NUMBER() OVER()) as id if you don't have one)"}

      _ ->
        :ok
    end
  end

  defp build_field_defs(columns) do
    columns
    |> Enum.with_index()
    |> Enum.map(fn {{name, pg_type}, idx} ->
      if String.downcase(name) == "id" do
        %{
          "name" => name,
          "type" => "text",
          "required" => true,
          "unique" => false,
          "indexed" => false,
          "system" => true,
          "sort_order" => idx,
          "options" => %{}
        }
      else
        %{
          "name" => name,
          "type" => lazy_type(pg_type),
          "required" => false,
          "unique" => false,
          "indexed" => false,
          "system" => false,
          "sort_order" => idx,
          "options" => %{}
        }
      end
    end)
  end

  # Maps a Postgres column type to the closest LazyPock field type. Unknown
  # types fall back to `json` (PocketBase defaults unknown view columns to a
  # JSON field too).
  defp lazy_type(pg_type) do
    case String.downcase(pg_type) do
      t when t in ~w(text character varying character char citext uuid) ->
        "text"

      t when t in ~w(numeric integer bigint smallint double precision real money) ->
        "number"

      "boolean" ->
        "bool"

      t when t in ~w(date timestamp timestamp without time zone timestamp with time zone) ->
        "date"

      t when t in ~w(json jsonb) ->
        "json"

      _ ->
        "json"
    end
  end

  # Rebuilds the select list from the introspected columns, casting a
  # non-text `id` column to TEXT (PocketBase's normalizeViewQueryId parity) so
  # record lookups / filters with string ids work. Column names are quoted and
  # re-aliased verbatim to preserve case.
  defp select_list_sql(columns) do
    columns
    |> Enum.map(fn {name, pg_type} ->
      quoted = TypeMapper.quote_ident(name)

      if String.downcase(name) == "id" and
           not MapSet.member?(@text_types, String.downcase(pg_type)) do
        "CAST(#{quoted} AS TEXT) AS #{quoted}"
      else
        "#{quoted} AS #{quoted}"
      end
    end)
    |> Enum.join(", ")
  end

  defp build_create_view_sql(name, query, columns) do
    "CREATE VIEW #{TypeMapper.quote_ident(name)} AS SELECT #{select_list_sql(columns)} FROM (#{query}) AS _v"
  end

  # ── Comment / literal scrubbing (for the checks only — never the query) ──

  # Produces a scrubbed copy of the query for the `;` / `*` checks: comments
  # (line `--` and block `/* */`) are removed and the contents of single-quoted
  # string literals are masked to `''`. The original query is never modified.
  defp scrub(sql) do
    do_scrub(sql, [], :normal)
  end

  defp do_scrub(<<>>, acc, _state), do: acc |> Enum.reverse() |> IO.iodata_to_binary()

  defp do_scrub(<<"--", rest::binary>>, acc, :normal), do: skip_line(rest, acc)
  defp do_scrub(<<"/*", rest::binary>>, acc, :normal), do: skip_block(rest, acc)
  defp do_scrub(<<"'", rest::binary>>, acc, :normal), do: do_scrub(rest, ["''" | acc], :string)
  defp do_scrub(<<"'", rest::binary>>, acc, :string), do: do_scrub(rest, ["''" | acc], :normal)
  defp do_scrub(<<_c, rest::binary>>, acc, :string), do: do_scrub(rest, acc, :string)

  defp do_scrub(<<c, rest::binary>>, acc, :normal) do
    do_scrub(rest, [<<c>> | acc], :normal)
  end

  defp skip_line(<<"\n", rest::binary>>, acc), do: do_scrub(rest, ["\n" | acc], :normal)
  defp skip_line(<<_c, rest::binary>>, acc), do: skip_line(rest, acc)
  defp skip_line(<<>>, acc), do: acc |> Enum.reverse() |> IO.iodata_to_binary()

  defp skip_block(<<"*/", rest::binary>>, acc), do: do_scrub(rest, [" " | acc], :normal)
  defp skip_block(<<_c, rest::binary>>, acc), do: skip_block(rest, acc)
  defp skip_block(<<>>, acc), do: acc |> Enum.reverse() |> IO.iodata_to_binary()
end
