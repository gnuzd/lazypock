defmodule Lazypock.Schemas.FilterCompilerTest do
  use ExUnit.Case, async: true

  alias Lazypock.Schemas.FilterCompiler

  describe "compile/1 — basic comparisons" do
    test "field = string literal" do
      assert {:ok, {~s["title" = $1], ["hello"]}} =
               FilterCompiler.compile(~s[title = 'hello'])
    end

    test "field != string literal" do
      assert {:ok, {~s["title" != $1], ["hello"]}} =
               FilterCompiler.compile(~s[title != 'hello'])
    end

    test "field > integer" do
      assert {:ok, {~s["age" > $1], [42]}} = FilterCompiler.compile("age > 42")
    end

    test "field >= integer" do
      assert {:ok, {~s["age" >= $1], [42]}} = FilterCompiler.compile("age >= 42")
    end

    test "field < integer" do
      assert {:ok, {~s["age" < $1], [42]}} = FilterCompiler.compile("age < 42")
    end

    test "field <= integer" do
      assert {:ok, {~s["age" <= $1], [42]}} = FilterCompiler.compile("age <= 42")
    end

    test "field ~ string (ILIKE with %% wrapping)" do
      assert {:ok, {~s["title" ILIKE $1], ["%hello%"]}} =
               FilterCompiler.compile(~s[title ~ 'hello'])
    end

    test "field !~ string (NOT ILIKE with %% wrapping)" do
      assert {:ok, {~s["title" NOT ILIKE $1], ["%hello%"]}} =
               FilterCompiler.compile(~s[title !~ 'hello'])
    end
  end

  describe "compile/1 — logical operators" do
    test "AND (&&) between two comparisons" do
      assert {:ok, {sql, params}} = FilterCompiler.compile(~s[a = '1' && b = '2'])
      assert sql =~ ~s["a" = $1]
      assert sql =~ ~s["b" = $2]
      assert sql =~ "AND"
      assert params == ["1", "2"]
    end

    test "camelCase field identifiers are lowercased to DB column names" do
      assert {:ok, {sql, params}} = FilterCompiler.compile(~s[tagColor = 'red'])
      assert sql =~ ~s["tagcolor" = $1]
      refute sql =~ "tagColor"
      assert params == ["red"]
    end

    test "OR (||) between two comparisons" do
      assert {:ok, {sql, params}} = FilterCompiler.compile(~s[a = '1' || b = '2'])
      assert sql =~ "OR"
      assert params == ["1", "2"]
    end

    test "AND takes precedence over OR" do
      assert {:ok, {sql, _params}} =
               FilterCompiler.compile(~s[a = '1' && b = '2' || c = '3'])

      # AND evaluated first, so: (a=1 AND b=2) OR c=3
      assert sql =~ ~r/\(.*AND.*\)\s*OR/
    end

    test "NOT (!) negates a comparison" do
      assert {:ok, {sql, params}} = FilterCompiler.compile(~s[!a = '1'])
      assert sql =~ "NOT"
      assert sql =~ ~s["a" = $1]
      assert params == ["1"]
    end
  end

  describe "compile/1 — parentheses grouping" do
    test "simple parenthesized expression" do
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(~s[(a = '1')])
      assert sql =~ ~s["a" = $1]
      assert params == ["1"]
    end

    test "parens override precedence: a && (b || c)" do
      assert {:ok, {sql, _params}} =
               FilterCompiler.compile(~s[a = '1' && (b = '2' || c = '3')])
      assert sql =~ ~r/AND\s*\(/
      assert sql =~ ~r/OR/
    end

    test "nested parens: (a || b) && c" do
      assert {:ok, {sql, _params}} =
               FilterCompiler.compile(~s[(a = '1' || b = '2') && c = '3'])
      assert sql =~ ~r/\(.*OR.*\)\s*AND/
    end

    test "three-level nesting" do
      assert {:ok, {sql, _params}} =
               FilterCompiler.compile(~s[(a = '1' && (b = '2' || c = '3'))])
      assert sql =~ ~r/AND/
      assert sql =~ ~r/OR/
    end
  end

  describe "compile/1 — literal types" do
    test "string literal with spaces" do
      assert {:ok, {_sql, ["hello world"]}} =
               FilterCompiler.compile(~s[title = 'hello world'])
    end

    test "integer literal" do
      assert {:ok, {_sql, [42]}} = FilterCompiler.compile("count = 42")
    end

    test "float literal" do
      assert {:ok, {_sql, [%Decimal{} = dec]}} = FilterCompiler.compile("price = 3.14")
      assert Decimal.equal?(dec, Decimal.new("3.14"))
    end

    test "boolean true literal" do
      assert {:ok, {_sql, [true]}} = FilterCompiler.compile("active = true")
    end

    test "boolean false literal" do
      assert {:ok, {_sql, [false]}} = FilterCompiler.compile("active = false")
    end

    test "null literal" do
      assert {:ok, {_sql, [nil]}} = FilterCompiler.compile("deleted_at = null")
    end

    test "boolean case variants — True" do
      assert {:ok, {_sql, [true]}} = FilterCompiler.compile("flag = True")
    end

    test "boolean case variants — TRUE" do
      assert {:ok, {_sql, [true]}} = FilterCompiler.compile("flag = TRUE")
    end

    test "boolean case variants — False" do
      assert {:ok, {_sql, [false]}} = FilterCompiler.compile("flag = False")
    end

    test "null case variants — Null" do
      assert {:ok, {_sql, [nil]}} = FilterCompiler.compile("val = Null")
    end

    test "null case variants — NULL" do
      assert {:ok, {_sql, [nil]}} = FilterCompiler.compile("val = NULL")
    end
  end

  describe "compile/1 — field names" do
    test "simple field name" do
      assert {:ok, {~s["title"], []}} = FilterCompiler.compile("title")
    end

    test "dotted field name with @ — fails because . is not in field name regex" do
      # The Enforcer resolves @request.auth.* tokens to literals before
      # passing to FilterCompiler, so the compiler never sees these raw.
      # Dots are not valid in field name identifiers.
      assert {:error, _} = FilterCompiler.compile("@request.auth.id")
    end

    test "field with underscore" do
      assert {:ok, {~s["my_field"], []}} = FilterCompiler.compile("my_field")
    end

    test "field starting with underscore" do
      assert {:ok, {~s["_hidden"], []}} = FilterCompiler.compile("_hidden")
    end

    test "@request.auth.id != empty string — fails because . is not valid" do
      # The Enforcer resolves @request.auth.* tokens before passing to
      # FilterCompiler, so this raw form is never seen by the compiler.
      assert {:error, _} = FilterCompiler.compile(~s[@request.auth.id != ''])
    end

    test "@request.auth.role = literal — fails because . is not valid" do
      # The Enforcer resolves these tokens before FilterCompiler.
      assert {:error, _} = FilterCompiler.compile(~s[@request.auth.role = 'admin'])
    end
  end

  describe "compile/1 — empty and edge inputs" do
    test "empty string returns ok with empty clause" do
      assert {:ok, {"", []}} = FilterCompiler.compile("")
    end

    test "whitespace-only string returns ok with empty clause" do
      assert {:ok, {"", []}} = FilterCompiler.compile("   ")
    end

    test "newline and tabs are trimmed" do
      assert {:ok, {"", []}} = FilterCompiler.compile("\n\t  ")
    end

    test "mising operator value returns error" do
      assert {:error, _} = FilterCompiler.compile("a =")
    end

    test "two tokens with no operator" do
      result = FilterCompiler.compile("a b")
      assert match?({:error, _}, result) or match?({:ok, _}, result)
    end

    test "unclosed parenthesis returns error" do
      assert {:error, _} = FilterCompiler.compile(~s[(a = '1'])
    end

    test "operator with no left operand" do
      result = FilterCompiler.compile("= 'value'")
      assert {:error, _} = result
    end
  end

  describe "compile/1 — error cases" do
    test "non-string input raises FunctionClauseError" do
      assert_raise FunctionClauseError, fn ->
        FilterCompiler.compile(nil)
      end
    end

    test "garbage input returns error" do
      assert {:error, _} = FilterCompiler.compile("^^^")
    end

    test "unclosed string literal is parsed as field name" do
      # The tokenizer splits on = and spaces, so 'unclosed becomes a token.
      # It falls through to the field name regex and matches as a field.
      assert {:ok, {_sql, _params}} = FilterCompiler.compile(~s[a = 'unclosed])
    end
  end

  describe "compile/1 — parameter numbering" do
    test "single param is $1" do
      {:ok, {sql, _params}} = FilterCompiler.compile(~s[a = 'x'])
      assert sql =~ "$1"
      refute sql =~ "$2"
    end

    test "two params are $1 and $2" do
      {:ok, {sql, params}} = FilterCompiler.compile(~s[a = 'x' && b = 'y'])
      assert sql =~ "$1"
      assert sql =~ "$2"
      assert params == ["x", "y"]
    end

    test "three params in complex expression" do
      {:ok, {sql, params}} =
        FilterCompiler.compile(~s[a = 'x' || (b = 'y' && c = 'z')])
      assert sql =~ "$1"
      assert sql =~ "$2"
      assert sql =~ "$3"
      assert length(params) == 3
    end

    test "params preserve order of appearance" do
      {:ok, {_sql, params}} =
        FilterCompiler.compile(~s[a = 'first' && b = 'second'])
      assert params == ["first", "second"]
    end

    test "ILIKE param has %% wrapping" do
      {:ok, {_sql, params}} = FilterCompiler.compile(~s[title ~ 'hello'])
      assert params == ["%hello%"]
    end

    test "NOT ILIKE param has %% wrapping" do
      {:ok, {_sql, params}} = FilterCompiler.compile(~s[title !~ 'hello'])
      assert params == ["%hello%"]
    end
  end

  describe "compile/1 — boolean and null comparisons" do
    test "field = true" do
      assert {:ok, {sql, [true]}} = FilterCompiler.compile("active = true")
      assert sql =~ "$1"
    end

    test "field = false" do
      assert {:ok, {sql, [false]}} = FilterCompiler.compile("active = false")
      assert sql =~ "$1"
    end

    test "field = null" do
      assert {:ok, {sql, [nil]}} = FilterCompiler.compile("deleted_at = null")
      assert sql =~ "$1"
    end

    test "field != null" do
      assert {:ok, {sql, [nil]}} = FilterCompiler.compile("deleted_at != null")
      assert sql =~ "!="
      assert sql =~ "$1"
    end
  end

  describe "compile/1 — standalone expressions" do
    test "standalone boolean true" do
      assert {:ok, {sql, [true]}} = FilterCompiler.compile("true")
      assert sql =~ "$1"
    end

    test "standalone boolean false" do
      assert {:ok, {_sql, [false]}} = FilterCompiler.compile("false")
    end

    test "standalone string literal as field value" do
      result = FilterCompiler.compile("hello")
      assert match?({:ok, _}, result)
    end
  end

  describe "compile/1 — multi-placeholder renumbering (regression: #38)" do
    # https://github.com/gnuzd/lazypock/issues/38 — the Enforcer resolves
    # @request.auth.* tokens to string literals, so any auth-token comparison
    # becomes a literal-vs-literal clause emitting TWO placeholders ($1 op $2).
    # When such a clause is compiled at index >= 2, all placeholders must be
    # renumbered — previously only $1 was rewritten, corrupting the SQL.

    test "literal-vs-literal as SECOND clause renumbers both placeholders" do
      # Rule: owner_id = @request.auth.id && @request.auth.role = 'admin'
      # resolved by the Enforcer to: owner_id = 'user-123' && 'superuser' = 'admin'
      # Buggy output was ("owner_id" = $1 AND $2 = $2) → bind error / 500.
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(
                 ~s[owner_id = 'user-123' && 'superuser' = 'admin']
               )

      assert sql == ~s[("owner_id" = $1 AND $2 = $3)]
      assert params == ["user-123", "superuser", "admin"]
    end

    test "literal-vs-literal clause at index >= 3 renumbers both placeholders" do
      # Rule: @request.auth.role = 'admin' || @request.auth.role = 'board'
      # Buggy output was ($1 = $2 OR $3 = $2) → $2 silently re-binds to 'admin',
      # so a 'board' user's rule evaluated against the wrong value.
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(
                 ~s['superuser' = 'admin' || 'superuser' = 'board']
               )

      assert sql == ~s[($1 = $2 OR $3 = $4)]
      assert params == ["superuser", "admin", "superuser", "board"]
    end

    test "literal-vs-literal deep inside a nested expression" do
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(~s[a = '1' || (b = '2' && 'x' = 'y')])

      assert sql == ~s[("a" = $1 OR ("b" = $2 AND $3 = $4))]
      assert params == ["1", "2", "x", "y"]
    end
  end

  describe "apply/3 — integration with SQL queries" do
    test "empty filter leaves query unchanged" do
      {sql, params} = FilterCompiler.apply("SELECT * FROM t", "")
      assert sql == "SELECT * FROM t"
      assert params == []
    end

    test "non-empty filter adds WHERE clause" do
      {sql, params} = FilterCompiler.apply("SELECT * FROM t", ~s[x = 'y'])
      assert sql == "SELECT * FROM t WHERE \"x\" = $1"
      assert params == ["y"]
    end

    test "filter with AND" do
      {sql, _params} =
        FilterCompiler.apply("SELECT * FROM t", ~s[a = '1' && b = '2'])
      assert sql =~ "WHERE"
      assert sql =~ "AND"
    end

    test "apply with base params preserves them" do
      {_sql, params} =
        FilterCompiler.apply("SELECT * FROM t WHERE id = $1", ~s[x = 'y'], ["existing"])
      assert params == ["existing", "y"]
    end

    test "apply with invalid filter returns unchanged" do
      {sql, params} = FilterCompiler.apply("SELECT * FROM t", "a =")
      assert sql == "SELECT * FROM t"
      assert params == []
    end

    test "apply with nil filter raises FunctionClauseError" do
      assert_raise FunctionClauseError, fn ->
        FilterCompiler.apply("SELECT * FROM t", nil)
      end
    end
  end

  describe "compile/1 — complex real-world patterns" do
    test "PocketBase-style auth filter: @request.auth.id != ''" do
      # The Enforcer resolves @request.auth.* tokens before FilterCompiler.
      # When the tokenizer sees the dot in @request.auth.id, classify fails.
      assert {:error, _} = FilterCompiler.compile(~s[@request.auth.id != ''])
    end

    test "PocketBase-style: owner_id = @request.auth.id" do
      # The Enforcer resolves @request.auth.id to a literal before
      # passing to FilterCompiler, so the compiler sees: owner_id = 'user-123'
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(~s[owner_id = 'user-123'])
      assert sql =~ ~s["owner_id" = $1]
      assert params == ["user-123"]
    end

    test "OR of two field comparisons with different field names" do
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(~s[status = 'published' || status = 'draft'])
      assert params == ["published", "draft"]
      assert sql =~ "OR"
    end

    test "AND of three conditions" do
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(~s[a = '1' && b = '2' && c = '3'])
      assert params == ["1", "2", "3"]
      assert sql =~ "AND"
    end

    test "mixed comparison types in complex expression" do
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(
                 ~s[age > 18 && status = 'active' || role ~ 'admin']
               )
      assert params == [18, "active", "%admin%"]
      assert sql =~ ">"
      assert sql =~ "ILIKE"
      assert sql =~ "OR"
    end

    test "NOT with parenthesized expression" do
      assert {:ok, {sql, _params}} =
               FilterCompiler.compile(~s[!(status = 'draft' && owner_id = '1')])
      assert sql =~ "NOT"
      assert sql =~ "AND"
    end
  end
end
