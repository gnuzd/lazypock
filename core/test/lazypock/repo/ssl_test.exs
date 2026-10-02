defmodule Lazypock.Repo.SSLTest do
  # async: false — the PGSSLMODE test mutates process-global environment.
  use ExUnit.Case, async: false

  import ExUnit.CaptureLog

  alias Lazypock.Repo.SSL

  defp url(query \\ ""), do: "ecto://u:p@db.example.com:5432/app" <> query
  defp ssl(query), do: SSL.apply(url: url(query))[:ssl]

  describe "sslmode mapping" do
    test "disable turns TLS off" do
      assert ssl("?sslmode=disable") == false
    end

    test "require encrypts without certificate verification" do
      assert [verify: :verify_none] = ssl("?sslmode=require")
    end

    test "require verifies against sslrootcert when one is given" do
      opts = ssl("?sslmode=require&sslrootcert=/etc/ssl/root.crt")

      assert opts[:verify] == :verify_peer
      assert opts[:cacertfile] == ~c"/etc/ssl/root.crt"
    end

    test "verify-full verifies the peer with default hostname checking" do
      opts = ssl("?sslmode=verify-full&sslrootcert=/etc/ssl/root.crt")

      assert opts[:verify] == :verify_peer
      assert opts[:cacertfile] == ~c"/etc/ssl/root.crt"
      refute Keyword.has_key?(opts, :customize_hostname_check)
    end

    test "verify-ca verifies the peer but not the hostname" do
      opts = ssl("?sslmode=verify-ca&sslrootcert=/etc/ssl/root.crt")

      assert opts[:verify] == :verify_peer
      assert [match_fun: fun] = opts[:customize_hostname_check]
      assert fun.("reference", "presented") == true
    end

    test "verify-full falls back to the system CA store" do
      opts = ssl("?sslmode=verify-full")

      assert opts[:verify] == :verify_peer
      assert is_list(opts[:cacerts])
      assert opts[:cacerts] != []
    end

    test "client certificate parameters map to file options" do
      opts = ssl("?sslmode=verify-full&sslcert=/c.pem&sslkey=/k.pem&sslrootcert=/r.pem")

      assert opts[:certfile] == ~c"/c.pem"
      assert opts[:keyfile] == ~c"/k.pem"
    end

    test "sslmode is case-insensitive" do
      assert ssl("?sslmode=VERIFY-FULL&sslrootcert=/r.pem")[:verify] == :verify_peer
    end
  end

  describe "fail-loud behavior" do
    test "allow and prefer raise instead of silently downgrading to plaintext" do
      for mode <- ["allow", "prefer"] do
        assert_raise ArgumentError, ~r/no opportunistic TLS/, fn ->
          ssl("?sslmode=#{mode}")
        end
      end
    end

    test "an unknown sslmode raises" do
      assert_raise ArgumentError, ~r/unsupported sslmode/, fn -> ssl("?sslmode=bogus") end
    end

    test "an explicit :ssl option wins over the URL and warns" do
      explicit = [verify: :verify_none, cacerts: []]
      config = [url: url("?sslmode=require"), ssl: explicit]

      log =
        capture_log(fn ->
          assert SSL.apply(config)[:ssl] == explicit
        end)

      assert log =~ "explicit"
    end

    test "channel_binding=require is reported as unsupported" do
      log =
        capture_log(fn ->
          assert [verify: :verify_none] = ssl("?sslmode=require&channel_binding=require")
        end)

      assert log =~ "channel_binding"
    end

    test "an sslmode at the config level is honored" do
      assert SSL.apply(sslmode: "require", url: url())[:ssl] == [verify: :verify_none]
    end

    test "PGSSLMODE is honored when the URL omits sslmode" do
      System.put_env("PGSSLMODE", "require")
      on_exit(fn -> System.delete_env("PGSSLMODE") end)

      assert SSL.apply(url: url())[:ssl] == [verify: :verify_none]
    end
  end

  describe "no TLS parameters" do
    test "leaves the config untouched" do
      config = [url: url(), pool_size: 5]

      assert SSL.apply(config) == config
    end

    test "an unrelated query string does not enable TLS" do
      assert SSL.apply(url: url("?pool_size=5")) == [url: url("?pool_size=5")]
    end
  end
end
