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
        "geo" => "JSONB"
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

      for t <- ["text", "number", "bool", "email", "url", "date", "datetime", "select",
                "multi_select", "file", "multi_file", "json", "relation", "editor",
                "password", "geo"] do
        assert t in types
      end

      refute "autodate" in types
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

    test "unsupported types produce no default clause" do
      assert TypeMapper.default_sql(%{"default" => "x", "type" => "json"}) == ""
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
