defmodule Lazypock.Auth.OAuth2SessionStoreTest do
  # async: false — mutates global app env and inspects the shared ETS table.
  use ExUnit.Case, async: false

  alias Lazypock.Auth.OAuth2

  @table :lazypock_oauth2_sessions

  test "the session table is protected (only the owner process may write)" do
    assert :ets.info(@table, :protection) == :protected
  end

  test "store/take round-trips the app origin and consumes the state" do
    assert {:ok, _state} =
             OAuth2.store_session("mock", "users", "verifier", "state-rt", "http://app.test:3000")

    assert {:ok, "mock", "users", "verifier", "http://app.test:3000"} =
             OAuth2.take_session("state-rt")

    # single use
    assert {:error, :not_found} = OAuth2.take_session("state-rt")
  end

  test "an empty or unknown state is rejected" do
    assert {:error, :not_found} = OAuth2.take_session("")
    assert {:error, :not_found} = OAuth2.take_session("does-not-exist")
  end

  test "sweep deletes expired sessions" do
    Application.put_env(:lazypock, :oauth2_session_ttl_ms, 1)
    on_exit(fn -> Application.delete_env(:lazypock, :oauth2_session_ttl_ms) end)

    assert {:ok, _} = OAuth2.store_session("mock", "users", "verifier", "state-expired")
    Process.sleep(5)

    assert OAuth2.sweep_sessions() >= 1
    assert {:error, :not_found} = OAuth2.take_session("state-expired")
  end

  test "refuses new sessions once the store is at capacity" do
    Application.put_env(:lazypock, :oauth2_session_max, OAuth2.session_count())
    on_exit(fn -> Application.delete_env(:lazypock, :oauth2_session_max) end)

    assert {:error, :too_many_sessions} =
             OAuth2.store_session("mock", "users", "verifier", "state-full")
  end
end
