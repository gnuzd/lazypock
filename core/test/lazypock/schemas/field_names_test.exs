defmodule Lazypock.Schemas.FieldNamesTest do
  use ExUnit.Case, async: true

  alias Lazypock.Schemas.FieldNames

  defp users_collection do
    %{
      fields: [
        %{name: "email", type: "email"},
        %{name: "password_hash", type: "password"},
        %{name: "emailVisibility", type: "bool"},
        %{name: "verificationToken", type: "text"},
        %{name: "name", type: "text"}
      ]
    }
  end

  describe "row_to_api/2" do
    test "maps lowercase DB columns to camelCase metadata names" do
      row = %{
        "email" => "a@b.com",
        "password_hash" => "hash",
        "emailvisibility" => true,
        "verificationtoken" => "tok",
        "name" => "Jane"
      }

      assert FieldNames.row_to_api(row, users_collection()) == %{
               "email" => "a@b.com",
               "password_hash" => "hash",
               "emailVisibility" => true,
               "verificationToken" => "tok",
               "name" => "Jane"
             }
    end

    test "leaves unknown columns unchanged" do
      assert FieldNames.row_to_api(%{"weird_col" => 1}, users_collection()) == %{
               "weird_col" => 1
             }
    end
  end

  describe "attrs_to_columns/2" do
    test "maps camelCase metadata names to lowercase DB columns" do
      attrs = %{
        "email" => "a@b.com",
        "emailVisibility" => false,
        "verificationToken" => "tok"
      }

      assert FieldNames.attrs_to_columns(attrs, users_collection()) == %{
               "email" => "a@b.com",
               "emailvisibility" => false,
               "verificationtoken" => "tok"
             }
    end

    test "leaves unknown keys unchanged" do
      assert FieldNames.attrs_to_columns(%{"foo" => 1}, users_collection()) == %{"foo" => 1}
    end
  end
end
