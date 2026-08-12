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
    test "projects to only requested fields plus system keys" do
      assert DynamicView.project_fields(@item, "id,title") == %{
               "id" => "1",
               "title" => "hi",
               "collectionId" => "c1",
               "collectionName" => "posts",
               "created" => "2024-01-01T00:00:00Z",
               "updated" => "2024-01-02T00:00:00Z"
             }
    end

    test "drops non-requested, non-system fields" do
      projected = DynamicView.project_fields(@item, "title")

      refute Map.has_key?(projected, "count")
      refute Map.has_key?(projected, "active")
      assert projected["title"] == "hi"
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

    test "only requested system keys are kept even when not listed" do
      projected = DynamicView.project_fields(@item, "count")

      assert projected == %{
               "count" => 2,
               "id" => "1",
               "collectionId" => "c1",
               "collectionName" => "posts",
               "created" => "2024-01-01T00:00:00Z",
               "updated" => "2024-01-02T00:00:00Z"
             }
    end
  end
end
