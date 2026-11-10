cask "margin" do
  version "0.11.0"
  sha256 "2b812b33e0050f24dd097a1398506204928de2ec86cf837106deb63547c9e9fe"

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
