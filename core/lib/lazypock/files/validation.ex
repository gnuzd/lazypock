defmodule Lazypock.Files.Validation do
  @moduledoc """
  Server-side validation for uploaded files.

  Both the upload's HTTP `content-type` header and its filename are
  client-controlled, so neither can be trusted. This module decides whether a
  file may be stored:

    1. The extension must be in an explicit allowlist.
    2. The bytes must be *consistent* with that extension — verified with a
       magic-byte sniff for binary formats, and with a UTF-8 / JSON check for
       text formats.

  A mismatch (for example a PHP script named `avatar.png` or an executable
  named `report.pdf`) is rejected, so the stored file cannot masquerade as a
  different type. `validate/2` returns the server-determined MIME type to
  persist rather than the value supplied by the client.

  This module is deliberately pure (no disk or DB access) so it is cheap to
  test and safe to call on every upload.
  """

  import Bitwise, only: [band: 2]

  # Extension allowlist. The value is the canonical MIME type we persist.
  # NOTE: `.svg` is intentionally excluded — SVG is active XML content and a
  # stored+served SVG is a script-execution vector.
  @extensions %{
    ".jpg" => "image/jpeg",
    ".jpeg" => "image/jpeg",
    ".png" => "image/png",
    ".gif" => "image/gif",
    ".webp" => "image/webp",
    ".pdf" => "application/pdf",
    ".csv" => "text/csv",
    ".txt" => "text/plain",
    ".json" => "application/json",
    ".zip" => "application/zip",
    ".mp4" => "video/mp4",
    ".mp3" => "audio/mpeg"
  }

  @text_extensions ~w(.csv .txt .json)

  @zip_magics [
    <<0x50, 0x4B, 0x03, 0x04>>,
    <<0x50, 0x4B, 0x05, 0x06>>,
    <<0x50, 0x4B, 0x07, 0x08>>
  ]

  @doc "The allowlisted `extension => canonical MIME type` map."
  @spec allowed_extensions() :: %{String.t() => String.t()}
  def allowed_extensions, do: @extensions

  @doc """
  Validates an uploaded `filename` and its `binary` contents.

  Returns `{:ok, mime_type}` with the server-determined MIME type, or
  `{:error, reason}` where `reason` is one of:

    * `:extension_not_allowed` — extension absent or not allowlisted
    * `:content_mismatch` — binary magic bytes disagree with the extension
    * `:invalid_text` — a text extension whose bytes are not valid UTF-8/JSON
  """
  @spec validate(String.t(), binary()) :: {:ok, String.t()} | {:error, atom()}
  def validate(filename, binary) when is_binary(filename) and is_binary(binary) do
    ext = extension(filename)

    case Map.fetch(@extensions, ext) do
      :error ->
        {:error, :extension_not_allowed}

      {:ok, canonical} ->
        case sniff(binary) do
          {:ok, ^canonical} -> {:ok, canonical}
          {:ok, _other} -> {:error, :content_mismatch}
          :unknown -> validate_unrecognized(canonical, ext, binary)
        end
    end
  end

  @doc "Returns the lowercased extension of `filename` (including the dot), or `\"\"`."
  @spec extension(String.t()) :: String.t()
  def extension(filename) when is_binary(filename) do
    filename |> Path.extname() |> String.downcase()
  end

  @doc """
  Sniffs a MIME type from magic bytes. Returns `{:ok, mime_type}` for known
  binary formats and `:unknown` for text or unrecognized content.
  """
  @spec sniff(binary()) :: {:ok, String.t()} | :unknown
  def sniff(binary) when is_binary(binary) do
    cond do
      prefix?(binary, <<0xFF, 0xD8, 0xFF>>) -> {:ok, "image/jpeg"}
      prefix?(binary, <<0x89, ?P, ?N, ?G, 0x0D, 0x0A, 0x1A, 0x0A>>) -> {:ok, "image/png"}
      prefix?(binary, "GIF87a") or prefix?(binary, "GIF89a") -> {:ok, "image/gif"}
      webp?(binary) -> {:ok, "image/webp"}
      prefix?(binary, "%PDF-") -> {:ok, "application/pdf"}
      zip?(binary) -> {:ok, "application/zip"}
      mp4?(binary) -> {:ok, "video/mp4"}
      mp3?(binary) -> {:ok, "audio/mpeg"}
      true -> :unknown
    end
  end

  def sniff(_other), do: :unknown

  # ── Private helpers ───────────────────────────────────

  defp validate_unrecognized(canonical, ext, binary) do
    cond do
      ext not in @text_extensions -> {:error, :content_mismatch}
      not utf8_text?(binary) -> {:error, :invalid_text}
      ext == ".json" and not valid_json?(binary) -> {:error, :invalid_text}
      true -> {:ok, canonical}
    end
  end

  # Text uploads: must be valid UTF-8 and free of NUL bytes (a NUL is the
  # practical marker of a binary payload).
  defp utf8_text?(binary) do
    :binary.match(binary, <<0>>) == :nomatch and String.valid?(binary)
  end

  defp valid_json?(binary) do
    match?({:ok, _}, Jason.decode(binary))
  end

  defp prefix?(binary, prefix) do
    byte_size(binary) >= byte_size(prefix) and
      binary_part(binary, 0, byte_size(prefix)) == prefix
  end

  defp webp?(binary) do
    byte_size(binary) >= 12 and binary_part(binary, 0, 4) == "RIFF" and
      binary_part(binary, 8, 4) == "WEBP"
  end

  defp zip?(binary), do: Enum.any?(@zip_magics, &prefix?(binary, &1))

  defp mp4?(binary) do
    byte_size(binary) >= 12 and binary_part(binary, 4, 4) == "ftyp"
  end

  defp mp3?(binary) do
    prefix?(binary, "ID3") or
      (byte_size(binary) >= 2 and :binary.at(binary, 0) == 0xFF and
         band(:binary.at(binary, 1), 0xE0) == 0xE0)
  end
end
