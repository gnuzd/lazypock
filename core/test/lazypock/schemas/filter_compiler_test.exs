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
      assert {:ok, {~s["title" ILIKE $1 ESCAPE '\\'], ["%hello%"]}} =
               FilterCompiler.compile(~s[title ~ 'hello'])
    end

    test "field !~ string (NOT ILIKE with %% wrapping)" do
      assert {:ok, {~s["title" NOT ILIKE $1 ESCAPE '\\'], ["%hello%"]}} =
               FilterCompiler.compile(~s[title !~ 'hello'])
    end

    test "~ escapes LIKE metacharacters so they match literally (PocketBase parity)" do
      # `_` is a single-char LIKE wildcard and has no "explicit pattern" form,
      # so it is always escaped; `%` opts into pattern mode when unescaped.
      assert {:ok, {_sql, ["%a\\_b%"]}} = FilterCompiler.compile(~s[name ~ 'a_b'])
      assert {:ok, {_sql, ["%50\\_off%"]}} = FilterCompiler.compile(~s[name ~ '50_off'])
    end

    test "~ leaves an unescaped %% as an explicit author-supplied pattern" do
      # `name ~ 'a%b'` is a startsWith/endsWith pattern: it is NOT wrapped or
      # escaped, which is what makes `name ~ 'prefix%'` work.
      assert {:ok, {_sql, ["a%b"]}} = FilterCompiler.compile(~s[name ~ 'a%b'])
      assert {:ok, {_sql, ["%foo"]}} = FilterCompiler.compile(~s[name ~ '%foo'])
    end

    test "~ preserves pre-existing escape sequences instead of double-escaping" do
      assert {:ok, {_sql, ["%a\\%b%"]}} = FilterCompiler.compile(~s[name ~ 'a\\%b'])
      assert {:ok, {_sql, ["%a\\_b%"]}} = FilterCompiler.compile(~s[name ~ 'a\\_b'])
    end

    test "~ treats an explicit unescaped % as an author-supplied pattern" do
      # `name ~ 'prefix%'` stays a startsWith search: no extra wrapping.
      assert {:ok, {_sql, ["prefix%"]}} = FilterCompiler.compile(~s[name ~ 'prefix%'])
    end

    test "!~ escapes LIKE metacharacters in the value too" do
      assert {:ok, {_sql, ["%a\\_b%"]}} = FilterCompiler.compile(~s[name !~ 'a_b'])
    end

    test "~ bound params are escaped the same way as literals" do
      assert {:ok, {~s["name" ILIKE $1 ESCAPE '\\'], ["%a\\_b%"]}} =
               FilterCompiler.compile("name ~ $1", ["a_b"])
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

    test "camelCase field identifiers are emitted verbatim (matching the column)" do
      assert {:ok, {sql, params}} = FilterCompiler.compile(~s[tagColor = 'red'])
      assert sql =~ ~s["tagColor" = $1]
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
      # The compiler reports "no clause" for blank input; it cannot know
      # whether the caller meant the public "" rule or a blank rule field.
      # The Enforcer's classify_rule/1 treats whitespace-only rule *values* as
      # invalid and denies -- see the fail-closed tests in
      # test/lazypock/rules/enforcer_fail_closed_test.exs.
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
               FilterCompiler.compile(~s[owner_id = 'user-123' && 'superuser' = 'admin'])

      assert sql == ~s[("owner_id" = $1 AND $2 = $3)]
      assert params == ["user-123", "superuser", "admin"]
    end

    test "literal-vs-literal clause at index >= 3 renumbers both placeholders" do
      # Rule: @request.auth.role = 'admin' || @request.auth.role = 'board'
      # Buggy output was ($1 = $2 OR $3 = $2) → $2 silently re-binds to 'admin',
      # so a 'board' user's rule evaluated against the wrong value.
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(~s['superuser' = 'admin' || 'superuser' = 'board'])

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

  describe "compile/1 — fail-closed guards" do
    # These guard the class of bug where a non-empty filter failed to produce
    # SQL and callers treated an empty clause as "no restriction" (= allow).

    test "a dangling && is a parse error, not a silently dropped clause" do
      assert {:error, _} = FilterCompiler.compile(~s[a = 1 &&])
      assert {:error, _} = FilterCompiler.compile(~s[a = 1 && &&])
      assert {:error, _} = FilterCompiler.compile(~s[a = 1 && (])
    end

    test "a dangling || is a parse error, not a silently dropped clause" do
      assert {:error, _} = FilterCompiler.compile(~s[a = 1 ||])
      assert {:error, _} = FilterCompiler.compile(~s[a = 1 || ||])
    end

    test "comparisons with no field operand error instead of emitting an empty clause" do
      # These used to compile to {:ok, {"", []}} -- indistinguishable from
      # the public "" rule to callers that treat an empty clause as allow.
      assert {:error, _} = FilterCompiler.compile(~s['a' ~ 'b'])
      assert {:error, _} = FilterCompiler.compile(~s['a' !~ 'b'])
      assert {:error, _} = FilterCompiler.compile(~s[$1 ~ 'b'])
      assert {:error, _} = FilterCompiler.compile(~s['a' ?= 'b'])
    end

    test "a filter longer than 3500 characters is rejected (PocketBase parity)" do
      short = String.duplicate("a = 1 || ", 400)
      assert byte_size(short) > 3500
      assert {:error, msg} = FilterCompiler.compile(short)
      assert msg =~ "maximum length"
    end

    test "a filter with more than 200 expressions is rejected (PocketBase parity)" do
      many = 1..(200 + 5) |> Enum.map(&"f#{&1} = 1") |> Enum.join(" || ")
      assert {:error, msg} = FilterCompiler.compile(many)
      assert msg =~ "maximum of 200 expressions"
    end

    test "exactly 200 expressions still compiles" do
      at_limit = 1..200 |> Enum.map(&"f#{&1} = 1") |> Enum.join(" || ")
      assert {:ok, {_sql, _params}} = FilterCompiler.compile(at_limit)
    end

    test "an operator inside a quoted literal is rejected (documented limitation)" do
      # The tokenizer splits on operators regardless of quoting, so a literal
      # containing && / || / = cannot be expressed. Rejecting is fail-closed;
      # it never silently drops part of the value.
      assert {:error, _} = FilterCompiler.compile(~s[name = 'a&&b'])
      assert {:error, _} = FilterCompiler.compile(~s[name = 'a||b'])
    end

    test "a bare quote never crashes the compiler (fuzz regression)" do
      # Found by the property test: `classify/1` sees a lone `'` as a quoted
      # literal (it both starts and ends with a quote), and unescape_literal
      # then asked for a -1 length slice -> FunctionClauseError -> 500.
      for input <- ["'", "a = '", "' < 'x'", "a = ''''"] do
        assert match?({:ok, _}, FilterCompiler.compile(input)) or
                 match?({:error, _}, FilterCompiler.compile(input)),
               "compiling #{inspect(input)} did not return ok/error"
      end
    end

    test "invalid UTF-8 bytes never crash the compiler (fuzz regression)" do
      # String.to_charlist/1 raises UnicodeConversionError on invalid UTF-8;
      # LIKE pattern building must be byte-wise.
      for input <- [<<0xFF>>, <<0xC3>>, "name ~ '" <> <<0xFF>> <> "'", "a = '" <> <<0xFE>> <> "'"] do
        assert match?({:ok, _}, FilterCompiler.compile(input)) or
                 match?({:error, _}, FilterCompiler.compile(input)),
               "compiling #{inspect(input)} did not return ok/error"
      end

      assert {:ok, {_sql, [<<37, 255, 37>>]}} =
               FilterCompiler.compile("name ~ '" <> <<0xFF>> <> "'")
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

  describe "compile/1 — PocketBase ? operators (any/at least one of)" do
    # The `?` prefix means "any/at least one of" over an array-valued column
    # (multi_select / multi_file / multi-relation → TEXT[]). `ANY` OR-combines
    # the per-element predicate; the negated forms use `NOT (… ALL(…))`, which
    # is true when at least one element fails the predicate.

    test "?= emits `= ANY`" do
      assert {:ok, {~s[$1 = ANY("tags")], ["news"]}} =
               FilterCompiler.compile(~s[tags ?= 'news'])
    end

    test "?!= emits `NOT (= ALL)` (at least one differs)" do
      assert {:ok, {~s[NOT ($1 = ALL("tags"))], ["news"]}} =
               FilterCompiler.compile(~s[tags ?!= 'news'])
    end

    test "?~ wraps the value in % and matches any element via unnest/EXISTS" do
      assert {:ok,
              {~s[EXISTS (SELECT 1 FROM unnest("tags") AS x WHERE x ILIKE $1 ESCAPE '\\')],
               ["%news%"]}} = FilterCompiler.compile(~s[tags ?~ 'news'])
    end

    test "?!~ wraps the value in % and matches any non-matching element" do
      assert {:ok,
              {~s[EXISTS (SELECT 1 FROM unnest("tags") AS x WHERE x NOT ILIKE $1 ESCAPE '\\')],
               ["%news%"]}} = FilterCompiler.compile(~s[tags ?!~ 'news'])
    end

    test "?~ escapes LIKE metacharacters in the value" do
      assert {:ok, {_sql, ["%a\\_b%"]}} = FilterCompiler.compile(~s[tags ?~ 'a_b'])
    end

    test "?> emits `< ANY` (at least one element greater than)" do
      assert {:ok, {~s[$1 < ANY("scores")], [10]}} =
               FilterCompiler.compile("scores ?> 10")
    end

    test "?>= emits `<= ANY`" do
      assert {:ok, {~s[$1 <= ANY("scores")], [10]}} =
               FilterCompiler.compile("scores ?>= 10")
    end

    test "?< emits `> ANY` (at least one element less than)" do
      assert {:ok, {~s[$1 > ANY("scores")], [10]}} =
               FilterCompiler.compile("scores ?< 10")
    end

    test "?<= emits `>= ANY`" do
      assert {:ok, {~s[$1 >= ANY("scores")], [10]}} =
               FilterCompiler.compile("scores ?<= 10")
    end

    test "no whitespace around the operator" do
      assert {:ok, {~s[$1 = ANY("tags")], ["news"]}} =
               FilterCompiler.compile(~s[tags?='news'])
    end

    test "with a TEXT[] column the value is coerced to text" do
      types = %{"tags" => "TEXT[]"}

      assert {:ok, {~s[$1 = ANY("tags")], ["news"]}} =
               FilterCompiler.compile(~s[tags ?= 'news'], [], types)

      assert {:ok, {~s[$1 = ANY("tags")], ["42"]}} =
               FilterCompiler.compile("tags ?= 42", [], types)
    end

    test "a bound param is coerced to the array element text" do
      assert {:ok, {~s[$1 = ANY("tags")], ["user-123"]}} =
               FilterCompiler.compile(~s[tags ?= $1], ["user-123"], %{"tags" => "TEXT[]"})
    end

    test "a known scalar column fails closed" do
      types = %{"title" => "TEXT"}

      assert {:error, _} = FilterCompiler.compile(~s[title ?= 'x'], [], types)
      assert {:error, _} = FilterCompiler.compile(~s[title ?~ 'x'], [], types)
    end

    test "a known JSONB column fails closed" do
      assert {:error, _} = FilterCompiler.compile(~s[meta ?= 'x'], [], %{"meta" => "JSONB"})
    end

    test "combines with standard operators and numbers params correctly" do
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(
                 ~s[tags ?= 'news' && title ~ 'hello'],
                 [],
                 %{"tags" => "TEXT[]", "title" => "TEXT"}
               )

      assert sql == ~s[($1 = ANY("tags") AND "title" ILIKE $2::TEXT ESCAPE '\\')]
      assert params == ["news", "%hello%"]
    end

    test "logic and parens with ? operators" do
      assert {:ok, {sql, params}} =
               FilterCompiler.compile(~s[(tags ?= 'a' || tags ?= 'b') && title = 'x'])

      assert sql =~ "OR"
      assert sql =~ "AND"
      assert params == ["a", "b", "x"]
    end

    test "NOT still applies to a ? clause" do
      assert {:ok, {sql, params}} = FilterCompiler.compile(~s[!(tags ?= 'news')])
      assert sql == ~s[NOT $1 = ANY("tags")]
      assert params == ["news"]
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
               FilterCompiler.compile(~s[age > 18 && status = 'active' || role ~ 'admin'])

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
