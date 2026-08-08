defmodule Lazypock.Hooks.EventTest do
  use ExUnit.Case, async: true

  alias Lazypock.Hooks.Event

  describe "Event.next/1,2" do
    test "next/1 proceeds with the chain" do
      e = Event.put(%Event{event: :on_record_create}, :record, %{"title" => "hi"})
      assert {:ok, %Event{} = e2} = Event.next(e)
      assert e2.record == %{"title" => "hi"}
    end

    test "next/2 with a post_fun schedules after-work (PocketBase 'after e.next()')" do
      e = %Event{event: :on_record_create}
      assert {:after, %Event{} = e2, post_fun} = Event.next(e, fn _ -> :ok end)
      assert is_function(post_fun, 1)
      assert Event.record(e2) == nil
    end

    test "run_after/2 runs the scheduled post_fun" do
      e = Event.put(%Event{event: :on_record_create}, :record, %{"id" => "1"})

      assert :ok =
               Event.run_after(e, fn e ->
                 send(self(), {:ran, e.record["id"]})
                 :ok
               end)

      assert_received {:ran, "1"}
    end

    test "run_after/2 surfaces {:error, reason} from post_fun" do
      e = %Event{event: :on_record_create}
      assert {:error, :boom} = Event.run_after(e, fn _ -> {:error, :boom} end)
    end
  end

  describe "Event.put/get/record" do
    test "put/3 stores into data and mirrors :record into the struct field" do
      e = %Event{event: :on_record_create}
      e = Event.put(e, :record, %{"title" => "x"})
      assert Event.get(e, :record) == %{"title" => "x"}
      assert e.record == %{"title" => "x"}
    end

    test "put/3 does not mirror non-record keys" do
      e = %Event{event: :on_record_create}
      e = Event.put(e, :token, "abc")
      assert Event.get(e, :token) == "abc"
      refute Map.has_key?(e, :token)
    end
  end
end

defmodule Lazypock.Hooks.RegistryTest do
  use ExUnit.Case, async: false

  alias Lazypock.Hooks.{Registry, Event}

  setup do
    Registry.clear()
    on_exit(fn -> Registry.clear() end)
    :ok
  end

  test "dispatch runs handlers in registration order, chaining events" do
    Registry.register(
      :on_record_create,
      {:fun,
       fn e ->
         send(self(), :first)
         Event.next(e)
       end}
    )

    Registry.register(
      :on_record_create,
      {:fun,
       fn e ->
         send(self(), :second)
         Event.next(e)
       end}
    )

    assert {:ok, %Event{} = e} = Registry.dispatch(:on_record_create, %{record: %{}})
    assert_received :first
    assert_received :second
    assert %Event{} = e
  end

  test "handlers can mutate the event (PocketBase e.next() after mutation)" do
    Registry.register(
      :on_record_create,
      {:fun,
       fn e ->
         e = Event.put(e, :record, Map.put(e.record || %{}, "slug", "hello-world"))
         Event.next(e)
       end}
    )

    {:ok, %Event{} = e} = Registry.dispatch(:on_record_create, %{record: %{}})
    assert e.record["slug"] == "hello-world"
  end

  test "{:error, reason} aborts the chain and later handlers don't run" do
    # flush any mailbox leftovers from prior async:false tests
    flush_mailbox()

    Registry.register(:on_record_create, {:fun, fn e -> Event.next(e) end})
    Registry.register(:on_record_create, {:fun, fn _ -> {:error, :nope} end})

    Registry.register(
      :on_record_create,
      {:fun,
       fn _ ->
         send(self(), :should_not_run)
         Event.next(%Event{})
       end}
    )

    assert {:error, :nope} = Registry.dispatch(:on_record_create, %{})
    refute_received :should_not_run
  end

  defp flush_mailbox do
    receive do
      _ -> flush_mailbox()
    after
      0 -> :ok
    end
  end

  test "not calling e.next() (returning something else) stops the chain with invalid result" do
    Registry.register(:on_record_create, {:fun, fn _ -> :oops end})

    assert {:error, {:invalid_hook_result, _, :oops}} = Registry.dispatch(:on_record_create, %{})
  end

  test "raising inside a handler stops the chain (PocketBase 'throwing an error')" do
    Registry.register(:on_record_create, {:fun, fn _ -> raise "boom" end})
    assert {:error, {:hook_exception, _, _}} = Registry.dispatch(:on_record_create, %{})
  end

  test "collection filtering: handlers registered for a collection only fire for it" do
    Registry.register(:on_record_create, {:fun, fn e -> Event.next(e) end},
      collections: ["posts"]
    )

    Registry.register(
      :on_record_create,
      {:fun,
       fn e ->
         send(self(), :posts_only)
         Event.next(e)
       end}, collections: ["posts"])

    Registry.register(
      :on_record_create,
      {:fun,
       fn e ->
         send(self(), :always)
         Event.next(e)
       end}
    )

    {:ok, _} = Registry.dispatch(:on_record_create, %{}, "posts")
    assert_received :posts_only
    assert_received :always

    Registry.clear()

    Registry.register(
      :on_record_create,
      {:fun,
       fn e ->
         send(self(), :posts_only)
         Event.next(e)
       end}, collections: ["posts"])

    {:ok, _} = Registry.dispatch(:on_record_create, %{}, "articles")
    refute_received :posts_only
  end

  test "collect_only runs handlers but ignores aborts" do
    Registry.register(:on_record_enrich, {:fun, fn e -> Event.next(e) end})
    Registry.register(:on_record_enrich, {:fun, fn _ -> {:error, :ignored} end})

    assert {:ok, %Event{}} =
             Registry.dispatch(:on_record_enrich, %{record: %{}}, nil, collect_only: true)
  end

  test "run_after executes collected post_funs in order" do
    Registry.register(
      :on_record_create,
      {:fun,
       fn e ->
         Event.next(e, fn _ ->
           send(self(), :a)
           :ok
         end)
       end}
    )

    Registry.register(
      :on_record_create,
      {:fun,
       fn e ->
         Event.next(e, fn _ ->
           send(self(), :b)
           :ok
         end)
       end}
    )

    {:ok, _e, after_funs} = Registry.dispatch(:on_record_create, %{})
    assert :ok = Registry.run_after(after_funs, %Event{event: :on_record_create})
    assert_received :a
    assert_received :b
  end
end

defmodule Lazypock.Hooks.HookMacroTest do
  use ExUnit.Case, async: false

  alias Lazypock.Hooks.{Registry, Event}

  setup do
    Registry.clear()
    on_exit(fn -> Registry.clear() end)
    :ok
  end

  defmodule TestPostsHooks do
    use Lazypock.Hooks.Hook, collection: "posts"

    def on_record_create(%Event{} = e) do
      e = Event.put(e, :record, Map.put(e.record, "slug", "test-slug"))
      Event.next(e)
    end

    def on_record_validate(%Event{} = e) do
      if e.record["title"] == "" do
        {:error, "title required"}
      else
        Event.next(e)
      end
    end

    def on_bootstrap(%Event{} = e), do: Event.next(e)
  end

  test "__hook_registrations__/0 registers only defined handlers, scoped to the collection" do
    regs = TestPostsHooks.__hook_registrations__()

    assert {:on_record_create, {TestPostsHooks, :on_record_create}, [collections: ["posts"]]} in regs

    assert {:on_record_validate, {TestPostsHooks, :on_record_validate}, [collections: ["posts"]]} in regs

    assert {:on_bootstrap, {TestPostsHooks, :on_bootstrap}, [collections: ["posts"]]} in regs
    refute Enum.any?(regs, fn {ev, _, _} -> ev == :on_record_update end)
  end

  test "end-to-end: use Hook module fires for its collection" do
    TestPostsHooks.__hook_registrations__()
    |> Enum.each(fn {event, target, opts} -> Registry.register(event, target, opts) end)

    {:ok, %Event{} = e} =
      Registry.dispatch(:on_record_create, %{record: %{"title" => "x"}}, "posts")

    assert e.record["slug"] == "test-slug"

    # Other collection: hook not fired → record unchanged
    {:ok, %Event{} = e2} =
      Registry.dispatch(:on_record_create, %{record: %{"title" => "x"}}, "articles")

    assert e2.record["slug"] == nil
  end

  test "validate hook can abort" do
    TestPostsHooks.__hook_registrations__()
    |> Enum.each(fn {event, target, opts} -> Registry.register(event, target, opts) end)

    assert {:error, "title required"} =
             Registry.dispatch(:on_record_validate, %{record: %{"title" => ""}}, "posts")
  end
end
