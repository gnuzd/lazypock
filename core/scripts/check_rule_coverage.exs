# Enforces per-module coverage minimums for the access-control code path.
#
#   MIX_ENV=test mix test --cover 2>&1 | tee cover.txt
#   elixir scripts/check_rule_coverage.exs cover.txt
#
# Why a separate gate: Elixir's built-in `test_coverage: [summary: [threshold: N]]`
# is a *global* minimum, so high coverage in unrelated modules can mask a
# regression in the rule enforcer or filter compiler. These two modules decide
# who can read/write every record, so they get their own floor.
#
# The floors are a ratchet: they are set to the measured baseline (rounded
# down), not the aspirational 95% target, so the gate guards against regression
# today. Raise them as coverage improves -- never lower them to make a build
# pass.

minimums = %{
  "Lazypock.Rules.Enforcer" => 88.0,
  "Lazypock.Schemas.FilterCompiler" => 88.0
}

case System.argv() do
  [path] ->
    report = File.read!(path)

    # Rows look like: "|     89.19% | Lazypock.Rules.Enforcer             |"
    measured =
      report
      |> String.split("\n")
      |> Enum.flat_map(fn line ->
        case Regex.run(~r/^\|\s*([\d.]+)%\s*\|\s*([A-Za-z0-9_.]+)\s*\|\s*$/, line) do
          [_, pct, module] ->
            case Float.parse(pct) do
              {value, _} -> [{module, value}]
              :error -> []
            end

          _ ->
            []
        end
      end)
      |> Map.new()

    if measured == %{} do
      IO.puts(:stderr, "No coverage rows parsed from #{path} — did `mix test --cover` run?")
      System.halt(1)
    end

    failures =
      Enum.flat_map(minimums, fn {module, minimum} ->
        case Map.fetch(measured, module) do
          {:ok, value} when value >= minimum ->
            IO.puts("ok   #{module}: #{value}% (min #{minimum}%)")
            []

          {:ok, value} ->
            IO.puts("FAIL #{module}: #{value}% is below the #{minimum}% minimum")
            [{module, value, minimum}]

          :error ->
            IO.puts("FAIL #{module}: missing from the coverage report")
            [{module, nil, minimum}]
        end
      end)

    if failures == [] do
      IO.puts("Access-control coverage gate passed.")
    else
      IO.puts(
        :stderr,
        "Access-control coverage gate failed for: " <>
          Enum.map_join(failures, ", ", fn {m, v, min} -> "#{m} (#{v}% < #{min}%)" end)
      )

      System.halt(1)
    end

  _ ->
    IO.puts(:stderr, "usage: elixir scripts/check_rule_coverage.exs <coverage-report.txt>")
    System.halt(1)
end
