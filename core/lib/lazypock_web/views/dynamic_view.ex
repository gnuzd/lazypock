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
    |> Map.put("collectionId", collection.id)
    |> Map.put("collectionName", collection.name)
    |> rename_timestamps()
    |> strip_password_fields(collection)
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
