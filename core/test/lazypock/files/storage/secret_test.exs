defmodule Lazypock.Files.Storage.SecretTest do
  use ExUnit.Case, async: true

  alias Lazypock.Files.Storage.Secret

  test "round-trips a secret" do
    encrypted = Secret.encrypt("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY")

    assert Secret.encrypted?(encrypted)
    refute encrypted =~ "wJalrXUtnFEMI"
    assert {:ok, "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"} = Secret.decrypt(encrypted)
  end

  test "encrypt is idempotent and handles blanks" do
    encrypted = Secret.encrypt("a")
    assert Secret.encrypt(encrypted) == encrypted
    assert Secret.encrypt(nil) == nil
    assert Secret.encrypt("") == ""
  end

  test "ciphertext is randomised (a fresh IV each time)" do
    refute Secret.encrypt("same") == Secret.encrypt("same")
  end

  test "plaintext values pass through and tampered ciphertext fails" do
    assert {:ok, "plain"} = Secret.decrypt("plain")
    assert {:error, :cannot_decrypt} = Secret.decrypt("enc:not-base64!!")

    encrypted = Secret.encrypt("secret")
    <<prefix::binary-size(4), first::binary-size(1), rest::binary>> = encrypted
    replacement = if first == "A", do: "B", else: "A"
    tampered = prefix <> replacement <> rest
    assert {:error, :cannot_decrypt} = Secret.decrypt(tampered)
  end
end
