cask "antigravity-manager" do
  version :latest
  sha256 :no_check

  name "Antigravity Manager"
  desc "Lightweight Zero-Proxy Multi-Host Account Switcher & Local Telemetry Studio"
  homepage "https://github.com/lbjlaq/Antigravity-Manager/tree/antigravity-manager"

  on_macos do
    arch intel: "x64", arm: "aarch64"

    url "https://github.com/lbjlaq/Antigravity-Manager/releases/download/antigravity-manager/Antigravity.Manager_#{arch}.dmg"

    app "Antigravity Manager.app"
    binary "#{appdir}/Antigravity Manager.app/Contents/MacOS/am"

    postflight do
      system_command "/usr/bin/xattr",
                     args:         ["-rd", "com.apple.quarantine", "#{appdir}/Antigravity Manager.app"],
                     must_succeed: false
    end

    zap trash: [
      "~/Library/Application Support/com.antigravity.manager",
      "~/Library/Caches/com.antigravity.manager",
      "~/Library/Preferences/com.antigravity.manager.plist",
      "~/Library/Saved Application State/com.antigravity.manager.savedState",
    ]
  end

  on_linux do
    arch arm: "aarch64", intel: "amd64"

    url "https://github.com/lbjlaq/Antigravity-Manager/releases/download/antigravity-manager/Antigravity.Manager_#{arch}.AppImage"
    binary "Antigravity.Manager_#{arch}.AppImage", target: "antigravity-manager"

    preflight do
      set_permissions "#{staged_path}/Antigravity.Manager_#{arch}.AppImage", "0755"
    end
  end
end
