defmodule Lazypock.Files.S3.SigV4Test do
  use ExUnit.Case, async: true

  alias Lazypock.Files.S3.SigV4

  @access_key "AKIAIOSFODNN7EXAMPLE"
  @secret "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
  @datetime ~U[2013-05-24 00:00:00Z]

  @opts [access_key_id: @access_key, secret_access_key: @secret, datetime: @datetime]

  defp signature(headers) do
    headers
    |> Map.fetch!("authorization")
    |> then(&Regex.run(~r/Signature=([0-9a-f]+)/, &1))
    |> List.last()
  end

  test "GET object example from the AWS documentation" do
    headers =
      SigV4.sign("GET", "https://examplebucket.s3.amazonaws.com/test.txt", %{
        "range" => "bytes=0-9",
        "x-amz-content-sha256" => SigV4.empty_sha256(),
        "x-amz-date" => "20130524T000000Z"
      }, @opts)

    assert headers["authorization"] =~
             "Credential=#{@access_key}/20130524/us-east-1/s3/aws4_request"

    assert headers["authorization"] =~
             "SignedHeaders=host;range;x-amz-content-sha256;x-amz-date"

    assert signature(headers) ==
             "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
  end

  test "PUT object example from the AWS documentation" do
    payload_hash = "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"

    headers =
      SigV4.sign("PUT", "https://examplebucket.s3.amazonaws.com/test%24file.text", %{
        "date" => "Fri, 24 May 2013 00:00:00 GMT",
        "x-amz-content-sha256" => payload_hash,
        "x-amz-date" => "20130524T000000Z",
        "x-amz-storage-class" => "REDUCED_REDUNDANCY"
      }, Keyword.put(@opts, :payload_hash, payload_hash))

    assert headers["authorization"] =~
             "SignedHeaders=date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class"

    assert signature(headers) ==
             "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
  end

  test "format_datetime/1 produces the AWS timestamp" do
    assert SigV4.format_datetime(@datetime) == "20130524T000000Z"
  end

  test "sha256_file/1 matches an in-memory hash without buffering" do
    path = Path.join(System.tmp_dir!(), "sigv4-#{System.unique_integer([:positive])}.bin")
    data = :crypto.strong_rand_bytes(200_000)
    File.write!(path, data)

    assert SigV4.sha256_file(path) == SigV4.sha256_hex(data)

    File.rm!(path)
  end
end
