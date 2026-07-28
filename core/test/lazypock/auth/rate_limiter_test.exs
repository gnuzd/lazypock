defmodule Lazypock.Auth.RateLimiterTest do
  use ExUnit.Case, async: false

  alias Lazypock.Auth.RateLimiter

  setup do
    # Ensure table exists, then clear it before each test
    RateLimiter.ensure_table()
    :ets.delete_all_objects(:lazypock_rate_limits)
    :ok
  end

  describe "check_rate/3" do
    test "returns :ok when no attempts have been recorded" do
      assert :ok = RateLimiter.check_rate("10.0.0.1", "users", "a@b.com")
    end

    test "returns :ok after 5 failed attempts (under limit)" do
      for _ <- 1..5 do
        RateLimiter.record_attempt("10.0.0.1", "users", "a@b.com", :failure)
      end

      assert :ok = RateLimiter.check_rate("10.0.0.1", "users", "a@b.com")
    end

    test "returns {:error, :rate_limited} after 10 failed attempts" do
      for _ <- 1..10 do
        RateLimiter.record_attempt("10.0.0.1", "users", "a@b.com", :failure)
      end

      assert {:error, :rate_limited} = RateLimiter.check_rate("10.0.0.1", "users", "a@b.com")
    end
  end

  describe "record_attempt/4" do
    test "success clears the rate limit entry" do
      for _ <- 1..10 do
        RateLimiter.record_attempt("10.0.0.1", "users", "a@b.com", :failure)
      end

      assert {:error, :rate_limited} = RateLimiter.check_rate("10.0.0.1", "users", "a@b.com")

      RateLimiter.record_attempt("10.0.0.1", "users", "a@b.com", :success)

      assert :ok = RateLimiter.check_rate("10.0.0.1", "users", "a@b.com")
    end

    test "different IP addresses are isolated" do
      for _ <- 1..10 do
        RateLimiter.record_attempt("10.0.0.1", "users", "a@b.com", :failure)
      end

      assert {:error, :rate_limited} = RateLimiter.check_rate("10.0.0.1", "users", "a@b.com")
      assert :ok = RateLimiter.check_rate("10.0.0.2", "users", "a@b.com")
    end

    test "different emails are isolated" do
      for _ <- 1..10 do
        RateLimiter.record_attempt("10.0.0.1", "users", "a@b.com", :failure)
      end

      assert {:error, :rate_limited} = RateLimiter.check_rate("10.0.0.1", "users", "a@b.com")
      assert :ok = RateLimiter.check_rate("10.0.0.1", "users", "other@b.com")
    end
  end

  describe "ip_from_conn/1" do
    test "extracts IP from conn" do
      conn = %Plug.Conn{remote_ip: {127, 0, 0, 1}}
      assert "127.0.0.1" = RateLimiter.ip_from_conn(conn)
    end

    test "handles IPv6 localhost" do
      conn = %Plug.Conn{remote_ip: {0, 0, 0, 0, 0, 0, 0, 1}}
      assert "::1" = RateLimiter.ip_from_conn(conn)
    end
  end
end
