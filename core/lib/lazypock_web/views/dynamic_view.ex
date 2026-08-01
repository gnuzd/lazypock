defmodule LazypockWeb.DynamicView do
  @moduledoc """
  Helper functions for rendering records from dynamic collections
  in a PocketBase-compatible format.
  """

  alias Lazypock.Collections.Registry
  alias Lazypock.Schemas.FieldNames
  alias Lazypock.Schemas.GenericRecord

  @doc """
  Formats a list of records from a collection into PocketBase-compatible items,
  adding collection metadata to each record.
  """
  @spec format_items([map()], String.t()) :: [map()]
  def format_items(records, collection_name) do
    {:ok, collection} = Registry.get(collection_name)

    Enum.map(records, fn record ->
      record
      |> FieldNames.row_to_api(collection)
      |> Map.put("collectionId", collection.id)
      |> Map.put("collectionName", collection.name)
      |> rename_timestamps()
      |> strip_password_fields(collection)
    end)
  end

  @doc """
  Formats a single record.
  """
  @spec format_item(map(), String.t()) :: map()
  def format_item(nil, _collection_name), do: nil

  def format_item(record, collection_name) do
    {:ok, collection} = Registry.get(collection_name)

    record
    |> FieldNames.row_to_api(collection)
    |> Map.put("collectionId", collection.id)
    |> Map.put("collectionName", collection.name)
    |> rename_timestamps()
    |> strip_password_fields(collection)
  end

  @doc """
  Expands relation fields on a list of records.

  For each field in `expand_fields` (comma-separated), loads the related
  record from the target collection and adds it as `expand.{field_name}`
  on each record.

  Silently skips unknown fields, non-relation fields, missing collections,
  and missing related records.
  """
  @spec expand_records([map()], String.t() | nil, String.t()) :: [map()]
  def expand_records(records, nil, _collection_name), do: records
  def expand_records(records, "", _collection_name), do: records

  def expand_records(records, expand_fields, collection_name) do
    field_names = String.split(expand_fields, ",") |> Enum.map(&String.trim/1)
    {:ok, collection} = Registry.get(collection_name)
    relation_fields = find_relation_fields(collection, field_names)

    # Batch-fetch all related records across all input records
    # to avoid N+1 queries (e.g. 10 posts sharing one category)
    batch = batch_fetch_related(records, relation_fields)

    Enum.map(records, fn record ->
      expanded =
        Map.new(relation_fields, fn {field_name, target_collection} ->
          related_id = record[field_name]
          key = {target_collection, related_id}
          {field_name, Map.get(batch, key)}
        end)
        |> Enum.filter(fn {_, v} -> not is_nil(v) end)
        |> Map.new()

      if expanded == %{} do
        record
      else
        Map.put(record, "expand", expanded)
      end
    end)
  end

  defp find_relation_fields(collection, expand_fields) do
    expand_set = MapSet.new(expand_fields)

    (collection.fields || [])
    |> Enum.filter(fn f -> f.type == "relation" and MapSet.member?(expand_set, f.name) end)
    |> Enum.map(fn f -> {f.name, f.options["collection"]} end)
    |> Enum.filter(fn {_, target} -> is_binary(target) end)
  end

  # Batch-fetch all related records across the input records.
  # Groups unique (target_collection, id) pairs and issues one
  # IN query per target collection — avoids N+1.
  # Uses GenericRecord.all_where/3 for consistent data coercion.
  defp batch_fetch_related(records, relation_fields) do
    pairs =
      for record <- records,
          {field_name, target_collection} <- relation_fields,
          related_id = record[field_name],
          is_binary(related_id) and related_id != "",
          reduce: MapSet.new() do
        acc -> MapSet.put(acc, {target_collection, related_id})
      end
      |> Enum.to_list()

    if pairs == [] do
      %{}
    else
      pairs
      |> Enum.group_by(fn {target, _id} -> target end, fn {_target, id} -> id end)
      |> Enum.flat_map(fn {target, ids} ->
        case Registry.get(target) do
          {:ok, _coll} ->
            placeholders =
              ids |> Enum.with_index(1) |> Enum.map(fn {_id, i} -> "$#{i}" end) |> Enum.join(", ")

            # Convert string UUIDs to binary for uuid column matching
            id_bins =
              Enum.map(ids, fn id ->
                case Ecto.UUID.dump(id) do
                  {:ok, bin} -> bin
                  :error -> id
                end
              end)

            GenericRecord.all_where(target, "id IN (#{placeholders})", id_bins)
            |> Enum.map(fn r -> {{target, r["id"]}, r} end)

          {:error, _} ->
            []
        end
      end)
      |> Map.new()
    end
  end

  @doc """
  Builds a paginated response matching PocketBase format.
  """
  @spec paginated_response([map()], non_neg_integer(), non_neg_integer(), non_neg_integer()) ::
          map()
  def paginated_response(items, total, page, per_page) do
    total_pages = if per_page > 0, do: ceil(total / per_page), else: 0

    %{
      "page" => page,
      "perPage" => per_page,
      "totalItems" => total,
      "totalPages" => total_pages,
      "items" => items
    }
  end

  defp rename_timestamps(record) do
    record
    |> maybe_rename("created_at", "created")
    |> maybe_rename("updated_at", "updated")
  end

  # Strips password fields from API responses (security + PocketBase compat).
  defp strip_password_fields(record, collection) do
    password_fields =
      (collection.fields || [])
      |> Enum.filter(fn f -> f.type == "password" end)
      |> Enum.map(fn f -> f.name end)

    Map.drop(record, password_fields)
  end

  defp maybe_rename(record, old_key, new_key) do
    case Map.fetch(record, old_key) do
      {:ok, value} -> record |> Map.put(new_key, value) |> Map.delete(old_key)
      :error -> record
    end
  end
end
