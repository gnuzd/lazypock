defmodule LazypockWeb.DynamicViewTest do
  use ExUnit.Case, async: true

  alias LazypockWeb.DynamicView

  @item %{
    "id" => "1",
    "title" => "hi",
    "count" => 2,
    "active" => true,
    "collectionId" => "c1",
    "collectionName" => "posts",
    "created" => "2024-01-01T00:00:00Z",
    "updated" => "2024-01-02T00:00:00Z"
  }

  describe "project_fields/2" do
    test "projects to only the requested fields" do
      assert DynamicView.project_fields(@item, "id,title") == %{
               "id" => "1",
               "title" => "hi"
             }
    end

    test "drops non-requested fields including system keys" do
      projected = DynamicView.project_fields(@item, "title")

      assert projected == %{"title" => "hi"}
      refute Map.has_key?(projected, "count")
      refute Map.has_key?(projected, "active")
      refute Map.has_key?(projected, "id")
      refute Map.has_key?(projected, "created")
      refute Map.has_key?(projected, "updated")
      refute Map.has_key?(projected, "collectionId")
      refute Map.has_key?(projected, "collectionName")
    end

    test "nil returns the item unchanged" do
      assert DynamicView.project_fields(@item, nil) == @item
    end

    test "star returns the item unchanged" do
      assert DynamicView.project_fields(@item, "*") == @item
    end

    test "trims whitespace in the field list" do
      assert DynamicView.project_fields(@item, " id , title ") ==
               DynamicView.project_fields(@item, "id,title")
    end

    test "system keys are returned only when explicitly requested" do
      assert DynamicView.project_fields(@item, "count") == %{"count" => 2}

      assert DynamicView.project_fields(@item, "id,created,updated") == %{
               "id" => "1",
               "created" => "2024-01-01T00:00:00Z",
               "updated" => "2024-01-02T00:00:00Z"
             }
    end

    test "unknown field names are silently ignored" do
      assert DynamicView.project_fields(@item, "title,nonexistent") == %{"title" => "hi"}
    end
  end
end
