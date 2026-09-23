defmodule Lazypock.Schema.ViewBuilderTest do
  use ExUnit.Case, async: true

  alias Lazypock.Collections.Collection
  alias Lazypock.Collections.Field
  alias Lazypock.Schema.ViewBuilder

  defp field(name, type \\ "text", opts \\ %{}, extra \\ %{}) do
    struct!(%Field{name: name, type: type, options: opts}, extra)
  end

  defp relation(name, target, max_select) do
    field(name, "relation", %{"collection" => target, "maxSelect" => max_select})
  end

  defp collection(name, fields) do
    %Collection{name: name, type: "base", fields: fields}
  end

  defp lines(list), do: Enum.join(list, "\n")

  describe "base columns" do
    test "injects the id column and preserves casing verbatim" do
      orders =
        collection("orders", [field("is_active", "bool"), field("isActive", "bool")])

      spec = %{
        "source" => "orders",
        "fields" => [%{"name" => "is_active"}, %{"name" => "isActive"}]
      }

      assert {:ok, sql} = ViewBuilder.to_query(spec, [orders])

      assert sql ==
               lines([
                 "SELECT",
                 "    \"orders\".\"id\" AS \"id\",",
                 "    \"orders\".\"is_active\" AS \"is_active\",",
                 "    \"orders\".\"isActive\" AS \"isActive\"",
                 "FROM \"orders\""
               ])
    end

    test "does not duplicate an explicitly selected id" do
      orders = collection("orders", [field("title")])

      spec = %{
        "source" => "orders",
        "fields" => [%{"name" => "id"}, %{"name" => "title"}]
      }

      assert {:ok, sql} = ViewBuilder.to_query(spec, [orders])

      assert sql ==
               "SELECT\n    \"orders\".\"id\" AS \"id\",\n    \"orders\".\"title\" AS \"title\"\nFROM \"orders\""
    end

    test "always keeps the base id as the view id even when another field claims it" do
      orders = collection("orders", [relation("customer", "customers", 1)])
      customers = collection("customers", [field("name")])

      spec = %{
        "source" => "orders",
        "relations" => [%{"alias" => "t1", "field" => "customer"}],
        "fields" => [%{"source" => "t1", "name" => "id", "as" => "id"}]
      }

      assert {:ok, sql} = ViewBuilder.to_query(spec, [orders, customers])

      assert sql ==
               lines([
                 "SELECT",
                 "    \"orders\".\"id\" AS \"id\",",
                 "    \"t1\".\"id\" AS \"id_2\"",
                 "FROM \"orders\"",
                 "LEFT JOIN \"customers\" AS \"t1\" ON \"t1\".\"id\"::text = \"orders\".\"customer\""
               ])
    end
  end

  describe "relations" do
    test "single relation becomes a LEFT JOIN with a text-cast id match" do
      orders = collection("orders", [relation("customer", "customers", 1)])
      customers = collection("customers", [field("name"), field("email")])

      spec = %{
        "source" => "orders",
        "relations" => [%{"alias" => "t1", "field" => "customer"}],
        "fields" => [
          %{"source" => "orders", "name" => "id"},
          %{"source" => "t1", "name" => "name"}
        ]
      }

      assert {:ok, sql} = ViewBuilder.to_query(spec, [orders, customers])

      assert sql ==
               lines([
                 "SELECT",
                 "    \"orders\".\"id\" AS \"id\",",
                 "    \"t1\".\"name\" AS \"customer_name\"",
                 "FROM \"orders\"",
                 "LEFT JOIN \"customers\" AS \"t1\" ON \"t1\".\"id\"::text = \"orders\".\"customer\""
               ])
    end

    test "multi relation becomes a jsonb_agg subquery and emits no JOIN" do
      orders = collection("orders", [relation("authors", "authors", 3)])
      authors = collection("authors", [field("name")])

      spec = %{
        "source" => "orders",
        "relations" => [%{"alias" => "t2", "field" => "authors"}],
        "fields" => [%{"source" => "t2", "name" => "name"}]
      }

      assert {:ok, sql} = ViewBuilder.to_query(spec, [orders, authors])

      assert sql ==
               lines([
                 "SELECT",
                 "    \"orders\".\"id\" AS \"id\",",
                 "    (SELECT COALESCE(jsonb_agg(\"_agg\".\"name\" ORDER BY \"_agg\".\"id\"), '[]'::jsonb) FROM \"authors\" AS \"_agg\" WHERE \"_agg\".\"id\"::text = ANY(\"orders\".\"authors\")) AS \"authors_name\"",
                 "FROM \"orders\""
               ])

      refute sql =~ "LEFT JOIN"
    end

    test "honors a custom output alias" do
      orders = collection("orders", [relation("customer", "customers", 1)])
      customers = collection("customers", [field("name")])

      spec = %{
        "source" => "orders",
        "relations" => [%{"alias" => "t1", "field" => "customer"}],
        "fields" => [%{"source" => "t1", "name" => "name", "as" => "authorName"}]
      }

      assert {:ok, sql} = ViewBuilder.to_query(spec, [orders, customers])
      assert sql =~ ~s("t1"."name" AS "authorName")
    end

    test "supports a self-relation aliased to the same table" do
      orders = collection("orders", [relation("parent", "orders", 1)])

      spec = %{
        "source" => "orders",
        "relations" => [%{"alias" => "t1", "field" => "parent"}],
        "fields" => [%{"source" => "t1", "name" => "id", "as" => "parent_id"}]
      }

      assert {:ok, sql} = ViewBuilder.to_query(spec, [orders])
      assert sql =~ ~s(LEFT JOIN "orders" AS "t1" ON "t1"."id"::text = "orders"."parent")
    end

    test "de-duplicates output names deterministically" do
      orders =
        collection("orders", [
          field("customer_name"),
          relation("customer", "customers", 1)
        ])

      customers = collection("customers", [field("name")])

      spec = %{
        "source" => "orders",
        "relations" => [%{"alias" => "t1", "field" => "customer"}],
        "fields" => [%{"name" => "customer_name"}, %{"source" => "t1", "name" => "name"}]
      }

      assert {:ok, sql} = ViewBuilder.to_query(spec, [orders, customers])
      assert sql =~ ~s("orders"."customer_name" AS "customer_name")
      assert sql =~ ~s("t1"."name" AS "customer_name_2")
    end
  end

  describe "sort and limit" do
    test "qualifies base columns and honors direction" do
      orders = collection("orders", [field("created_at", "date"), field("name")])

      spec = %{
        "source" => "orders",
        "fields" => [%{"name" => "name"}],
        "sort" => "-created_at, name",
        "limit" => 50
      }

      assert {:ok, sql} = ViewBuilder.to_query(spec, [orders])

      assert sql ==
               lines([
                 "SELECT",
                 "    \"orders\".\"id\" AS \"id\",",
                 "    \"orders\".\"name\" AS \"name\"",
                 "FROM \"orders\"",
                 "ORDER BY \"orders\".\"created_at\" DESC, \"orders\".\"name\" ASC",
                 "LIMIT 50"
               ])
    end

    test "omits ORDER BY / LIMIT when not provided" do
      orders = collection("orders", [field("name")])
      spec = %{"source" => "orders", "fields" => [%{"name" => "name"}]}

      assert {:ok, sql} = ViewBuilder.to_query(spec, [orders])
      refute sql =~ "ORDER BY"
      refute sql =~ "LIMIT"
    end
  end

  describe "validation errors" do
    setup do
      orders =
        collection("orders", [field("title"), field("secret", "text", %{}, %{hidden: true})])

      customers = collection("customers", [field("name")])

      %{orders: orders, customers: customers}
    end

    test "requires a source collection", %{orders: orders} do
      assert {:error, message} = ViewBuilder.to_query(%{}, [orders])
      assert message =~ "source collection is required"
    end

    test "rejects an unknown source collection", %{orders: orders} do
      assert {:error, message} = ViewBuilder.to_query(%{"source" => "nope"}, [orders])
      assert message =~ "source collection 'nope' does not exist"
    end

    test "rejects an unknown base field", %{orders: orders} do
      spec = %{"source" => "orders", "fields" => [%{"name" => "missing"}]}
      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "field 'missing' does not exist on 'orders'"
    end

    test "rejects hidden and password fields", %{orders: orders} do
      spec = %{"source" => "orders", "fields" => [%{"name" => "secret"}]}
      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "is hidden"

      password = collection("users", [field("password", "password")])
      spec = %{"source" => "users", "fields" => [%{"name" => "password"}]}
      assert {:error, message} = ViewBuilder.to_query(spec, [password])
      assert message =~ "is a password field"
    end

    test "rejects a non-relation field used as a relation", %{orders: orders} do
      spec = %{
        "source" => "orders",
        "relations" => [%{"alias" => "t1", "field" => "title"}],
        "fields" => [%{"name" => "title"}]
      }

      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "is not a relation"
    end

    test "rejects a relation whose target collection is missing" do
      orders = collection("orders", [relation("customer", "gone", 1)])

      spec = %{
        "source" => "orders",
        "relations" => [%{"alias" => "t1", "field" => "customer"}],
        "fields" => [%{"name" => "customer"}]
      }

      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "relation target 'gone' does not exist"
    end

    test "rejects an unknown field source", %{orders: orders} do
      spec = %{"source" => "orders", "fields" => [%{"source" => "t9", "name" => "name"}]}
      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "unknown field source 't9'"
    end

    test "rejects an invalid relation alias", %{orders: orders} do
      spec = %{
        "source" => "orders",
        "relations" => [%{"alias" => "1bad", "field" => "title"}],
        "fields" => [%{"name" => "title"}]
      }

      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "relation alias '1bad'"
    end

    test "rejects a relation alias that collides with the source name", %{orders: orders} do
      spec = %{
        "source" => "orders",
        "relations" => [%{"alias" => "orders", "field" => "title"}],
        "fields" => [%{"name" => "title"}]
      }

      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "conflicts with the source collection name"
    end

    test "rejects duplicate relation aliases", %{customers: customers} do
      orders = collection("orders", [relation("customer", "customers", 1)])

      spec = %{
        "source" => "orders",
        "relations" => [
          %{"alias" => "t1", "field" => "customer"},
          %{"alias" => "t1", "field" => "customer"}
        ],
        "fields" => [%{"name" => "customer"}]
      }

      assert {:error, message} = ViewBuilder.to_query(spec, [orders, customers])
      assert message =~ "duplicate relation alias 't1'"
    end

    test "rejects an invalid output alias", %{orders: orders} do
      spec = %{"source" => "orders", "fields" => [%{"name" => "title", "as" => "1bad"}]}
      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "output column name '1bad'"

      spec = %{"source" => "orders", "fields" => [%{"name" => "title", "as" => 42}]}
      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "output column name must be a string"
    end

    test "rejects a sort on an unknown field", %{orders: orders} do
      spec = %{
        "source" => "orders",
        "fields" => [%{"name" => "title"}],
        "sort" => "nope"
      }

      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "sort field 'nope'"
    end

    test "rejects invalid limits", %{orders: orders} do
      spec = %{"source" => "orders", "fields" => [%{"name" => "title"}], "limit" => 0}
      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "limit must be a positive integer"

      spec = %{"source" => "orders", "fields" => [%{"name" => "title"}], "limit" => "10"}
      assert {:error, message} = ViewBuilder.to_query(spec, [orders])
      assert message =~ "limit must be a positive integer"
    end
  end
end
