cask "antigravity-tools" do
  version "3.3.15"
  sha256 :no_check

  url "https://github.com/lbjlaq/Antigravity-Manager/releases/download/v#{version}/Antigravity.Tools_#{version}_universal.dmg"
  name "Antigravity Tools"
  desc "Professional Account Management for AI Services"
  homepage "https://github.com/lbjlaq/Antigravity-Manager"

  app "Antigravity Tools.app"

  zap trash: [
    "~/Library/Application Support/com.lbjlaq.antigravity-tools",
    "~/Library/Caches/com.lbjlaq.antigravity-tools",
    "~/Library/Preferences/com.lbjlaq.antigravity-tools.plist",
    "~/Library/Saved Application State/com.lbjlaq.antigravity-tools.savedState",
  ]

  caveats <<~EOS
    If you encounter the "App is damaged" error, please run the following command:
      sudo xattr -rd com.apple.quarantine "/Applications/Antigravity Tools.app"
    
    Or install with the --no-quarantine flag:
      brew install --cask --no-quarantine antigravity-tools
  EOS
end
