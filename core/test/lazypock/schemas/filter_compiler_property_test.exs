defmodule Lazypock.Schemas.FilterCompilerPropertyTest do
  use ExUnit.Case, async: true
  use ExUnitProperties

  alias Lazypock.Schemas.FilterCompiler

  # Property-based coverage for the filter compiler.
  #
  # The compiler has no public AST -- `compile/3` returns `{sql, params}` -- so
  # the properties are stated over its observable contract:
  #
  #   1. arbitrary bytes never crash it
  #   2. generated well-formed filters never crash it
  #   3. placeholders and bound params always line up (the #38 bug class:
  #      a clause emitting two placeholders that is not renumbered either
  #      leaves a hole or silently re-binds an earlier parameter)
  #   4. user values are always bound, never interpolated into the SQL text

  @fields ~w(title status count owner_id tagColor my_field id)
  @words ~w(hello admin a_b x foo bar basename)
  @ops ~w(= != ~ !~ > >= < <=)

  defp comparison_gen do
    gen all(
          field <- member_of(@fields),
          op <- member_of(@ops),
          value <- one_of([member_of(@words), integer()])
        ) do
      "#{field} #{op} '#{value}'"
    end
  end

  defp filter_gen do
    tree(comparison_gen(), fn child ->
      one_of([
        gen all(left <- child, right <- child, op <- member_of(["&&", "||"])) do
          "(#{left} #{op} #{right})"
        end,
        gen all(inner <- child) do
          "!#{inner}"
        end
      ])
    end)
  end

  defp compile_outcome(filter) do
    try do
      FilterCompiler.compile(filter)
    rescue
      error -> {:raised, error}
    catch
      kind, reason -> {:caught, kind, reason}
    end
  end

  # Iteration count is tunable so PR runs stay fast while the nightly job can
  # fuzz deeper: PROPERTY_MAX_RUNS=2000 MIX_ENV=test mix test <this file>
  defp property_max_runs do
    case Integer.parse(System.get_env("PROPERTY_MAX_RUNS", "300")) do
      {n, _} when n > 0 -> n
      _ -> 300
    end
  end

  property "arbitrary bytes never crash the parser" do
    check all(input <- binary(), max_runs: property_max_runs()) do
      outcome = compile_outcome(input)

      assert match?({:ok, _}, outcome) or match?({:error, _}, outcome),
             "compiling #{inspect(input)} did not return ok/error: #{inspect(outcome)}"
    end
  end

  property "generated well-formed filters never crash the compiler" do
    check all(filter <- filter_gen(), max_runs: property_max_runs()) do
      outcome = compile_outcome(filter)

      assert match?({:ok, _}, outcome) or match?({:error, _}, outcome),
             "compiling #{inspect(filter)} did not return ok/error: #{inspect(outcome)}"
    end
  end

  property "placeholders and bound params always line up" do
    check all(filter <- filter_gen(), max_runs: property_max_runs()) do
      case FilterCompiler.compile(filter) do
        {:ok, {sql, params}} ->
          numbers =
            ~r/\$(\d+)/
            |> Regex.scan(sql)
            |> Enum.map(fn [_, n] -> String.to_integer(n) end)

          assert numbers != [], "no placeholders emitted for #{inspect(filter)}"

          assert Enum.sort(numbers) == Enum.to_list(1..length(numbers)),
                 "placeholders #{inspect(numbers)} are not contiguous for #{inspect(filter)}"

          assert length(params) == length(numbers),
                 "param/placeholder mismatch for #{inspect(filter)}: " <>
                   "#{length(params)} params vs #{inspect(numbers)}"

        {:error, _} ->
          :ok
      end
    end
  end

  property "an empty or blank filter yields the documented empty clause" do
    check all(
            blanks <- list_of(member_of([" ", "\t", "\n"]), min_length: 0, max_length: 8),
            max_runs: property_max_runs()
          ) do
      assert {:ok, {"", []}} = FilterCompiler.compile(Enum.join(blanks))
    end
  end

  property "user values are bound as parameters, never interpolated" do
    check all(value <- member_of(@words), max_runs: property_max_runs()) do
      {:ok, {sql, params}} = FilterCompiler.compile("title = '#{value}'")

      assert params == [value]

      refute String.contains?(sql, value),
             "value #{inspect(value)} leaked into the SQL text: #{sql}"
    end
  end

  property "LIKE values are escaped so metacharacters cannot widen the match" do
    check all(value <- member_of(@words), max_runs: property_max_runs()) do
      {:ok, {_sql, [pattern]}} = FilterCompiler.compile("title ~ '#{value}'")

      # An unescaped `%` anywhere but the auto-added wrapping would let the
      # value act as a pattern; every `%`/`_` inside the value must be escaped.
      inner = String.slice(pattern, 1, byte_size(pattern) - 2)

      escaped_metachars =
        inner
        |> String.replace("\\%", "")
        |> String.replace("\\_", "")

      refute String.contains?(escaped_metachars, "%")
      refute String.contains?(escaped_metachars, "_")
    end
  end
end
