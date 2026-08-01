defmodule Lazypock.Schemas.FieldNames do
  @moduledoc """
  Normalizes field names between the three representations:

  * **Metadata names** (PocketBase-compatible) — e.g. `emailVisibility`,
    `verificationToken`. These come from `_fields.name` and are what the
    client library (lazypock-ts / PocketBase SDK) sends and expects.
  * **API key names** — same as metadata names (camelCase).
  * **DB column names** — Postgres folds unquoted identifiers to lowercase,
    so a column declared as `emailVisibility` is physically stored as
    `emailvisibility`.

  Reads: `row_to_api(record, collection)` maps DB column keys back to the
  metadata (camelCase) names so API responses are PocketBase-compatible.
  Writes: `attrs_to_columns(attrs, collection)` maps incoming camelCase keys
  to the actual (lowercase) DB column names before building SQL.
  """

  @doc """
  Maps a record map (keys as returned by Postgres — lowercased) to API keys
  (collection metadata names). Unknown DB columns pass through unchanged.
  """
  @spec row_to_api(map(), map()) :: map()
  def row_to_api(record, collection) do
    column_to_name = column_to_metadata_map(collection)

    Map.new(record, fn {key, value} ->
      {Map.get(column_to_name, key, key), value}
    end)
  end

  @doc """
  Maps incoming attrs (API keys = metadata names) to actual DB column names.
  Unknown keys pass through unchanged.
  """
  @spec attrs_to_columns(map(), map()) :: map()
  def attrs_to_columns(attrs, collection) do
    name_to_column = metadata_to_column_map(collection)

    Map.new(attrs, fn {key, value} ->
      {Map.get(name_to_column, key, key), value}
    end)
  end

  # ── Helpers ─────────────────────────────────────────

  defp column_to_metadata_map(collection) do
    Map.new(collection.fields || [], fn field ->
      {String.downcase(field.name), field.name}
    end)
  end

  defp metadata_to_column_map(collection) do
    Map.new(collection.fields || [], fn field ->
      {field.name, String.downcase(field.name)}
    end)
  end
end
