defmodule Lazypock.Schemas.FieldNames do
  @moduledoc """
  Bridges the API field names and the physical PostgreSQL column names.

  LazyPock keeps field names **verbatim** end to end: `_fields.name`, the API
  key and the physical column are the same string. A field entered (or
  imported) as `field_name` stays `field_name`; one entered as `fieldName`
  stays `fieldName`. The DDL engine declares columns with quoted identifiers
  (`"fieldName"`) so PostgreSQL does not fold the case away, and the migration
  `align_columns_and_system_timestamps` renames legacy lowercased columns to
  match.

  Reads: `row_to_api(record, collection)` maps DB column keys to API keys.
  Writes: `attrs_to_columns(attrs, collection)` maps incoming API keys to the
  actual DB column names before building SQL.

  Both directions are identity today because the three representations are
  the same string. The seam is kept so callers don't need to change if a
  collection ever needs an explicit column alias.
  """

  @doc """
  Maps a record map (keys as returned by Postgres) to API keys.
  """
  @spec row_to_api(map(), map()) :: map()
  def row_to_api(record, _collection) when is_map(record), do: record

  @doc """
  Maps incoming attrs (API keys) to actual DB column names.
  """
  @spec attrs_to_columns(map(), map()) :: map()
  def attrs_to_columns(attrs, _collection) when is_map(attrs), do: attrs
end
