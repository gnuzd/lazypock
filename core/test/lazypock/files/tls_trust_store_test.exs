defmodule Lazypock.Files.TlsTrustStoreTest do
  use ExUnit.Case, async: true

  # Outbound HTTPS — the S3/R2 adapter and OAuth2 both go through
  # Req → Finch → Mint — needs a CA trust store. A release image ships no OS
  # `ca-certificates`, so Mint falls back to `:castore`; without it every
  # request raises "default CA trust store not available" and file uploads
  # 500. This fails loudly if the castore dependency is ever dropped.
  test "a CA trust store is available for outbound TLS" do
    path = CAStore.file_path()

    assert is_binary(path)
    assert File.exists?(path)
  end
end
