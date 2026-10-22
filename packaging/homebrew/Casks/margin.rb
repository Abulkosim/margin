cask "margin" do
  version "0.4.0"
  sha256 "d0a4a782c2491b6d8cd2790a7a9794c5e4f8295c7eab98e5842bb215b72b5d69"

  url "https://github.com/Abulkosim/margin/releases/download/v#{version}/Margin_#{version}_universal.dmg"
  name "Margin"
  desc "Menu bar usage limits for Claude and Codex accounts"
  homepage "https://abulkosim.github.io/margin/"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates true
  depends_on macos: :monterey

  app "Margin.app"

  zap trash: [
    "~/Library/Application Support/com.abulkosim.margin",
    "~/Library/LaunchAgents/Margin.plist",
  ]

  caveats <<~EOS
    Margin is not notarized, so macOS blocks the first launch.
    Open System Settings → Privacy & Security, click Open Anyway, then Open.
    On macOS 12–14, Control-click Margin in Applications → Open also works.
  EOS
end
