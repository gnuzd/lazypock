defmodule LazypockWeb.DynamicView do
  @moduledoc """
  Helper functions for rendering records from dynamic collections
  in a PocketBase-compatible format.
  """

  alias Lazypock.Collections.Registry

  @doc """
  Formats a list of records from a collection into PocketBase-compatible items,
  adding collection metadata to each record.
  """
  @spec format_items([map()], String.t()) :: [map()]
  def format_items(records, collection_name) do
    {:ok, collection} = Registry.get(collection_name)

    Enum.map(records, fn record ->
      record
      |> Map.put("collectionId", collection.id)
      |> Map.put("collectionName", collection.name)
      |> format_timestamps()
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
    |> Map.put("collectionId", collection.id)
    |> Map.put("collectionName", collection.name)
    |> format_timestamps()
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

  defp format_timestamps(record) do
    record
    |> format_timestamp("created_at", "created")
    |> format_timestamp("updated_at", "updated")
  end

  defp format_timestamp(record, db_key, api_key) do
    case Map.get(record, db_key) do
      nil ->
        Map.put(record, api_key, nil)

      %DateTime{} = dt ->
        record |> Map.put(api_key, DateTime.to_iso8601(dt)) |> Map.delete(db_key)

      value when is_binary(value) ->
        record |> Map.put(api_key, value) |> Map.delete(db_key)

      _ ->
        record |> Map.put(api_key, nil) |> Map.delete(db_key)
    end
  end
end
