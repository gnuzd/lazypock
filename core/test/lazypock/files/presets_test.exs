defmodule Lazypock.Files.PresetsTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Files.Presets
  alias Lazypock.Settings

  setup do
    original = Settings.get()
    Presets.clear_cache()

    on_exit(fn ->
      Settings.put(original)
      Presets.clear_cache()
    end)

    :ok
  end

  defp put_presets(presets) do
    Settings.put(Map.put(Settings.get(), "image", %{"presets" => presets}))
    Presets.clear_cache()
  end

  test "ships thumb and content as eager defaults" do
    assert Presets.names() == ["thumb", "content"]
    assert Enum.map(Presets.eager(), & &1["name"]) == ["thumb", "content"]
    assert Presets.get("thumb")["fit"] == "cover"
    assert Presets.get("content")["width"] == 1280
  end

  test "settings override the defaults" do
    put_presets([%{"name" => "card", "width" => 640, "height" => nil, "fit" => "contain"}])

    assert Presets.names() == ["card"]
    assert Presets.get("card")["width"] == 640
    assert Presets.eager() == []
  end

  test "invalid presets are dropped rather than poisoning the list" do
    put_presets([
      %{"name" => "good", "width" => 100},
      %{"name" => "", "width" => 100},
      %{"name" => "no-width"},
      %{"name" => "bad-width", "width" => "wide"},
      "not-a-map"
    ])

    assert Presets.names() == ["good"]
  end

  test "a preset with a height defaults to cover, without to contain" do
    put_presets([
      %{"name" => "square", "width" => 50, "height" => 50},
      %{"name" => "wide", "width" => 50}
    ])

    assert Presets.get("square")["fit"] == "cover"
    assert Presets.get("wide")["fit"] == "contain"
  end

  test "for_size/1 only matches configured preset names" do
    assert Presets.for_size("thumb")["name"] == "thumb"
    assert Presets.for_size("480x720") == nil
    assert Presets.preset?("content")
    refute Presets.preset?("480x720")
  end
end
