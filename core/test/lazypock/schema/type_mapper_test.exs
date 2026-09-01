defmodule Lazypock.Schema.TypeMapperTest do
  use ExUnit.Case, async: true

  alias Lazypock.Schema.TypeMapper

  describe "to_pg_with_opts/2" do
    test "maps every supported type to its PostgreSQL column type" do
      expected = %{
        "text" => "TEXT",
        "number" => "NUMERIC",
        "bool" => "BOOLEAN",
        "email" => "TEXT",
        "url" => "TEXT",
        "date" => "TIMESTAMPTZ",
        "datetime" => "TIMESTAMPTZ",
        "select" => "TEXT",
        "multi_select" => "TEXT[]",
        "file" => "TEXT",
        "multi_file" => "TEXT[]",
        "json" => "JSONB",
        "relation" => "TEXT",
        "editor" => "TEXT",
        "password" => "TEXT",
        "geo" => "JSONB",
        "autodate" => "TIMESTAMPTZ"
      }

      for {type, pg} <- expected do
        assert TypeMapper.to_pg_with_opts(type, %{}) == pg, "type #{type}"
      end
    end

    test "relation with maxSelect > 1 maps to TEXT[] (multi-relation)" do
      assert TypeMapper.to_pg_with_opts("relation", %{"maxSelect" => 2}) == "TEXT[]"
      assert TypeMapper.to_pg_with_opts("relation", %{"maxSelect" => 5}) == "TEXT[]"
    end

    test "relation with maxSelect <= 1 maps to TEXT" do
      assert TypeMapper.to_pg_with_opts("relation", %{"maxSelect" => 1}) == "TEXT"
      assert TypeMapper.to_pg_with_opts("relation", %{"maxSelect" => 0}) == "TEXT"
    end

    test "relation without maxSelect maps to TEXT" do
      assert TypeMapper.to_pg_with_opts("relation", %{}) == "TEXT"
      assert TypeMapper.to_pg_with_opts("relation", nil) == "TEXT"
    end

    test "raises for unknown types" do
      assert_raise KeyError, fn -> TypeMapper.to_pg_with_opts("nope", %{}) end
    end
  end

  describe "valid_types/0 and valid_type?/1" do
    test "valid_types includes all supported strings" do
      types = TypeMapper.valid_types()

      for t <- [
            "text",
            "number",
            "bool",
            "email",
            "url",
            "date",
            "datetime",
            "select",
            "multi_select",
            "file",
            "multi_file",
            "json",
            "relation",
            "editor",
            "password",
            "geo",
            "autodate"
          ] do
        assert t in types
      end
    end

    test "valid_type? accepts known and rejects unknown" do
      assert TypeMapper.valid_type?("text")
      assert TypeMapper.valid_type?("multi_file")
      refute TypeMapper.valid_type?("weird")

      # Guard clauses: non-binary input raises FunctionClauseError
      assert_raise FunctionClauseError, fn -> TypeMapper.valid_type?(nil) end
      assert_raise FunctionClauseError, fn -> TypeMapper.valid_type?(:text) end
    end
  end

  describe "default_sql/1" do
    test "nil default produces no clause" do
      assert TypeMapper.default_sql(%{"default" => nil}) == ""
      assert TypeMapper.default_sql(%{}) == ""
      assert TypeMapper.default_sql(nil) == ""
    end

    test "text default is quoted and escaped" do
      assert TypeMapper.default_sql(%{"default" => "hello", "type" => "text"}) ==
               "DEFAULT 'hello'"

      assert TypeMapper.default_sql(%{"default" => "o'brien", "type" => "text"}) ==
               "DEFAULT 'o''brien'"
    end

    test "number default is unquoted" do
      assert TypeMapper.default_sql(%{"default" => 42, "type" => "number"}) == "DEFAULT 42"
      assert TypeMapper.default_sql(%{"default" => 4.5, "type" => "number"}) == "DEFAULT 4.5"
    end

    test "bool default maps to TRUE/FALSE" do
      assert TypeMapper.default_sql(%{"default" => true, "type" => "bool"}) == "DEFAULT TRUE"
      assert TypeMapper.default_sql(%{"default" => false, "type" => "bool"}) == "DEFAULT FALSE"
    end

    test "date default is quoted" do
      assert TypeMapper.default_sql(%{"default" => "2026-01-01", "type" => "date"}) ==
               "DEFAULT '2026-01-01'"
    end

    test "autodate with onCreate renders DEFAULT now()" do
      assert TypeMapper.default_sql(%{"type" => "autodate", "options" => %{"onCreate" => true}}) ==
               "DEFAULT now()"

      assert TypeMapper.default_sql(%{
               "type" => "autodate",
               "options" => %{"onCreate" => true, "onUpdate" => true}
             }) == "DEFAULT now()"
    end

    test "autodate without onCreate (update-only) has no default" do
      assert TypeMapper.default_sql(%{"type" => "autodate", "options" => %{"onUpdate" => true}}) ==
               ""

      assert TypeMapper.default_sql(%{"type" => "autodate", "options" => %{}}) == ""
      assert TypeMapper.default_sql(%{"type" => "autodate"}) == ""
    end

    test "unsupported types produce no default clause" do
      assert TypeMapper.default_sql(%{"default" => "x", "type" => "json"}) == ""
    end
  end

  describe "autodate_trigger?/2" do
    test "detects onCreate/onUpdate from raw field maps" do
      assert TypeMapper.autodate_trigger?(
               %{"type" => "autodate", "options" => %{"onCreate" => true}},
               :on_create
             )

      refute TypeMapper.autodate_trigger?(
               %{"type" => "autodate", "options" => %{"onCreate" => true}},
               :on_update
             )

      assert TypeMapper.autodate_trigger?(
               %{"type" => "autodate", "options" => %{"onCreate" => true, "onUpdate" => true}},
               :on_update
             )
    end

    test "detects triggers from Field structs (atom keys)" do
      field = %Lazypock.Collections.Field{
        type: "autodate",
        options: %{"onUpdate" => true}
      }

      assert TypeMapper.autodate_trigger?(field, :on_update)
      refute TypeMapper.autodate_trigger?(field, :on_create)
    end

    test "non-autodate types never trigger" do
      refute TypeMapper.autodate_trigger?(%{"type" => "date", "options" => %{}}, :on_create)
      refute TypeMapper.autodate_trigger?(%{"type" => "text"}, :on_update)
    end

    test "missing options never trigger" do
      refute TypeMapper.autodate_trigger?(%{"type" => "autodate"}, :on_create)
      refute TypeMapper.autodate_trigger?(%{"type" => "autodate", "options" => nil}, :on_create)
    end
  end

  describe "system_timestamp_columns/0" do
    test "returns the reserved system column names" do
      assert TypeMapper.system_timestamp_columns() == ["created_at", "updated_at"]
    end
  end

  describe "escape_string/1 and quote_ident/1" do
    test "escape_string doubles single quotes" do
      assert TypeMapper.escape_string("it's") == "it''s"
      assert TypeMapper.escape_string("plain") == "plain"
    end

    test "quote_ident wraps in double quotes" do
      assert TypeMapper.quote_ident("posts") == "\"posts\""
    end

    test "quote_ident escapes embedded double quotes" do
      assert TypeMapper.quote_ident("we\"ird") == "\"we\"\"ird\""
    end

    test "quote_ident is guarded to binaries" do
      assert_raise FunctionClauseError, fn -> TypeMapper.quote_ident(%{}) end
      assert_raise FunctionClauseError, fn -> TypeMapper.quote_ident(nil) end
    end
  end
end
