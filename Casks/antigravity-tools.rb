cask "antigravity-tools" do
  version "4.2.2"
  sha256 :no_check

  name "Antigravity Tools"
  desc "Professional Account Management for AI Services"
  homepage "https://github.com/lbjlaq/Antigravity-Manager"

  on_macos do
    url "https://github.com/lbjlaq/Antigravity-Manager/releases/download/v#{version}/Antigravity.Tools_#{version}_universal.dmg"

    app "Antigravity Tools.app"

    postflight do
      system_command "xattr",
                     args:         ["-rd", "com.apple.quarantine", "#{appdir}/Antigravity Tools.app"],
                     sudo:         false,
                     must_succeed: false
    end

    zap trash: [
      "~/Library/Application Support/com.lbjlaq.antigravity-tools",
      "~/Library/Caches/com.lbjlaq.antigravity-tools",
      "~/Library/Preferences/com.lbjlaq.antigravity-tools.plist",
      "~/Library/Saved Application State/com.lbjlaq.antigravity-tools.savedState",
    ]


  end

  on_linux do
    arch arm: "aarch64", intel: "amd64"

    url "https://github.com/lbjlaq/Antigravity-Manager/releases/download/v#{version}/Antigravity.Tools_#{version}_#{arch}.AppImage"
    binary "Antigravity.Tools_#{version}_#{arch}.AppImage", target: "antigravity-tools"

    preflight do
      system_command "/bin/chmod", args: ["+x", "#{staged_path}/Antigravity.Tools_#{version}_#{arch}.AppImage"]
    end
  end
end
