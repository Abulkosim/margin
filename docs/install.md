# Install Margin

Short version: [README → Install](../README.md#install). Downloads: [GitHub Releases](https://github.com/Abulkosim/margin/releases/latest).

Margin is free and open source. Builds are not yet signed with an Apple Developer ID or a Windows publisher certificate, so macOS and Windows warn on first launch. Updates are signed separately and verified in the app. Homebrew and winget packages are not available yet.

| Platform | File |
| --- | --- |
| macOS 12+ · Apple Silicon and Intel | `Margin_<version>_universal.dmg` |
| Windows x64 | `Margin_<version>_x64-setup.exe` (recommended) or `Margin_<version>_x64_en-US.msi` |

## macOS

These steps match macOS 15 Sequoia and macOS 26 Tahoe. Control-click → **Open** no longer skips the warning on these versions.

1. Download `Margin_<version>_universal.dmg`.
2. Open the DMG and drag **Margin** to **Applications**. `~/Applications` also works.
3. Open Margin from Applications.
4. macOS shows **“Margin” Not Opened**, saying Apple could not verify it is free of malware. Click **Done**. Do not click Move to Trash.
5. Open **System Settings → Privacy & Security**.
6. Scroll to **Security**. Next to **“Margin” was blocked to protect your Mac**, click **Open Anyway**.
7. Confirm with **Open** and enter your password or use Touch ID.

macOS remembers this choice; later launches open normally. Managed Macs may not allow exceptions. See [Apple: Safely open apps on your Mac](https://support.apple.com/en-us/102445).

On macOS 12–14, Control-click Margin in Applications → **Open** also works.

Fallback only if **Open Anyway** does not appear:

```sh
xattr -d com.apple.quarantine /Applications/Margin.app
```

This removes the download flag macOS uses to trigger the check. Use it only for a file you downloaded from this repository's releases and [verified](#verify-a-download).

Margin then asks nothing else. It has no Dock icon or app window; look for its icon in the menu bar. If the menu bar is full, the icon may be hidden behind the notch or by a menu bar manager.

## Windows

1. Download `Margin_<version>_x64-setup.exe` (or the `.msi`).
2. Run it. If **Windows protected your PC** appears, click **More info → Run anyway**.
3. The installer adds Margin for the current user. If WebView2 is missing, it [downloads Microsoft's setup program](https://v2.tauri.app/distribute/windows-installer/#downloaded-bootstrapper), so first installation may need internet access.
4. Margin appears in the system tray. Windows may put new icons in the overflow menu (**^**); drag it onto the taskbar to keep it visible.

A managed device may hide **Run anyway**. See [Microsoft Defender SmartScreen](https://learn.microsoft.com/en-us/windows/security/operating-system-security/virus-and-threat-protection/microsoft-defender-smartscreen/).

**Smart App Control.** On Windows 11 with Smart App Control on, unsigned apps are blocked with no Run anyway option. Microsoft offers [no per-app exception](https://support.microsoft.com/en-us/topic/what-is-smart-app-control-285ea03d-fa88-4d56-882e-6698afdb7003). You can turn Smart App Control off in **Windows Security → App & browser control**, or wait for a signed build. On older Windows 11 builds, turning it back on requires reinstalling Windows.

**Antivirus warnings.** New unsigned files have no reputation yet, so Microsoft Defender or another antivirus may flag one by mistake. Check the file against `SHA256SUMS.txt` and its attestation below. If it matches, report the false positive to [Microsoft's file submission page](https://www.microsoft.com/en-us/wdsi/filesubmission) (choose **Incorrectly detected as malware/malicious**) and [open an issue](https://github.com/Abulkosim/margin/issues/new/choose).

## Verify a download

Each release includes `SHA256SUMS.txt`. Compare your file's checksum with it:

```sh
shasum -a 256 Margin_<version>_universal.dmg
```

```powershell
Get-FileHash Margin_<version>_x64-setup.exe -Algorithm SHA256
```

Releases after v0.3.0 also carry GitHub build attestations and an SPDX SBOM (`margin-<version>.sbom.spdx.json`). With the [GitHub CLI](https://cli.github.com), check that a file was built by this repository's release workflow:

```sh
gh attestation verify Margin_<version>_universal.dmg --repo Abulkosim/margin
gh attestation verify Margin_<version>_x64-setup.exe --repo Abulkosim/margin
```

## Updates

Margin checks GitHub Releases for a new version once a day. Turn this off with **Check for updates daily** in **Settings → General → Updates**, or check by hand there. Installing is always your choice; Margin downloads, verifies, and restarts only when you click install.

Each update is signed with the project's update key, and Margin verifies the signature and version before installing. An altered or mismatched download is rejected and nothing is installed. Update signatures do not replace OS publisher signing, so a fresh manual install still shows the warnings above.

Update checks send no credentials, account details, or usage readings. Margin has no telemetry. See [PRIVACY.md](../PRIVACY.md).

v0.1.1 and earlier have no updater; install the latest release once by hand.

## Uninstall

First turn off **Launch at login** in Margin's Settings, then right-click the Margin icon and choose **Quit Margin**.

**macOS.** Delete `Margin.app` from Applications. To remove settings, last-known usage, and history, delete:

```
~/Library/Application Support/com.abulkosim.margin/
```

If Launch at login was left on, also delete `~/Library/LaunchAgents/Margin.plist`.

**Windows.** Open **Settings → Apps → Installed apps**, choose **Margin → Uninstall**. To remove settings, last-known usage, and history, delete:

```
%APPDATA%\com.abulkosim.margin\
```

`%LOCALAPPDATA%\com.abulkosim.margin\` holds the WebView cache and can be deleted too.

Uninstalling never touches Claude Code or Codex folders (`~/.claude`, `~/.codex`, or custom ones) or Keychain items. Your CLI sign-ins keep working.
