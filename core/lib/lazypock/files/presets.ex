defmodule Lazypock.Files.Presets do
  @moduledoc """
  Named image variants (thumbnails) configured in the Studio.

  Presets live under the `image` key of the settings document, e.g.

      {
        "image": {
          "presets": [
            {"name": "thumb",   "width": 100,  "height": 100, "fit": "cover",   "quality": 80, "eager": true},
            {"name": "content", "width": 1280, "height": null, "fit": "contain", "quality": 80, "eager": true}
          ]
        }
      }

  `fit` is one of `cover` (fill + centre-crop), `contain` (fit inside, never
  upscale) or `exact` (stretch). `eager` presets are generated during upload so
  their URLs are valid immediately; lazy ones are generated on first request.

  Defaults ship `thumb` and `content` (both eager). Preset changes never delete
  existing variants — regenerating is an explicit action.
  """

  alias Lazypock.Settings

  @cache_key {__MODULE__, :cache}
  @ttl_ms 2_000

  @defaults [
    %{
      "name" => "thumb",
      "width" => 100,
      "height" => 100,
      "fit" => "cover",
      "quality" => 80,
      "eager" => true
    },
    %{
      "name" => "content",
      "width" => 1280,
      "height" => nil,
      "fit" => "contain",
      "quality" => 80,
      "eager" => true
    }
  ]

  @fits ~w(cover contain exact)

  @doc "All configured presets (list of maps)."
  def all, do: cached()

  @doc "Preset names."
  def names, do: Enum.map(all(), & &1["name"])

  @doc "Presets marked `eager`."
  def eager, do: Enum.filter(all(), & &1["eager"])

  @doc "Look up a preset by name (or `nil`)."
  def get(name) when is_binary(name), do: Enum.find(all(), &(&1["name"] == name))
  def get(_name), do: nil

  @doc "Whether `name` is a configured preset."
  def preset?(name), do: not is_nil(get(name))

  @doc "The preset map for a scale request: a preset when known, else nil."
  def for_size(size), do: get(size)

  @doc false
  def clear_cache, do: :persistent_term.erase(@cache_key)

  defp cached do
    now = System.monotonic_time(:millisecond)

    case :persistent_term.get(@cache_key, nil) do
      {at, value} when now - at < @ttl_ms ->
        value

      _ ->
        value = load()
        :persistent_term.put(@cache_key, {now, value})
        value
    end
  end

  defp load do
    case Settings.get("image", %{}) do
      %{"presets" => presets} when is_list(presets) and presets != [] ->
        presets
        |> Enum.map(&normalize/1)
        |> Enum.reject(&is_nil/1)
        |> case do
          [] -> @defaults
          normalized -> normalized
        end

      _ ->
        @defaults
    end
  rescue
    _ -> @defaults
  end

  # Only well-formed presets are kept; invalid entries are dropped so a broken
  # Studio value cannot poison every upload.
  defp normalize(%{} = preset) do
    name = preset["name"]
    width = preset["width"]

    cond do
      not is_binary(name) or name == "" -> nil
      not is_integer(width) or width <= 0 -> nil
      true -> normalize_fit(preset, name, width)
    end
  end

  defp normalize(_), do: nil

  defp normalize_fit(preset, name, width) do
    fit = preset["fit"]
    height = preset["height"]

    fit =
      cond do
        fit in @fits -> fit
        is_integer(height) -> "cover"
        true -> "contain"
      end

    %{
      "name" => name,
      "width" => width,
      "height" => if(is_integer(height) and height > 0, do: height, else: nil),
      "fit" => fit,
      "quality" => if(is_integer(preset["quality"]), do: preset["quality"], else: 80),
      "eager" => preset["eager"] == true
    }
  end
end
