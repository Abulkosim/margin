# Native checks

Browser and unit tests cannot prove these. Run them on a real Mac and in a Windows VM, on a downloaded release build, and record the result below.

**If a check fails, label the failure; don't ship it as working.** Say so in the release notes and README. If Make active fails on a platform, turn it off on that platform. When its checks pass on a platform, remove its **Beta** label there (Settings, the confirmation dialog, README).

Use dedicated test accounts for checks 5 to 7, not sign-ins you depend on. Margin can only tell that it sent a notification; only you can see whether it appeared.

## Both platforms

1. **Test notification.** Settings → General → Notifications → **Send test notification**.
   Expected: a notification titled "Margin · Test notification" appears, and Settings says "Sent at …". This is the first check to run on each new macOS version, because the notification plugin uses Apple's deprecated `NSUserNotification` API, which reports no errors. If "Sent at …" shows and nothing appears after allowing Margin in the system's settings and turning off Focus or Do not disturb, record a failure: alerts are not reaching you on this OS.
2. **Permission off.** Turn Margin's notifications off in System Settings → Notifications (Windows: Settings → System → Notifications), then send a test.
   Expected: nothing appears. Margin still says "Sent at …", since desktop systems don't report this to it. Turn notifications back on.
3. **Real threshold or run-out alert.** Set **Notify at** to just below an account's current usage remaining, or use the account until it crosses a threshold.
   Expected: one notification such as "Work · Session 19% left" within one refresh, and no second one on later refreshes. For a run-out warning, use an account quickly with **Run-out warnings** on: "Work · Session runs out around 16:30". Also record a fast usage alert if one fires.
4. **Not-updating alert.** Needs a Claude account and a Codex account that both read fine, **Notify when usage stops updating** on, and Margin not sleeping. This blocks Claude Code on the machine during the test.
   - Block on Mac: `printf '0.0.0.0 api.anthropic.com\n:: api.anthropic.com\n' | sudo tee -a /etc/hosts`, then `sudo dscacheutil -flushcache; sudo killall -HUP mDNSResponder`.
   - Block on Windows: open Notepad as administrator, add `0.0.0.0 api.anthropic.com` and `:: api.anthropic.com` to `C:\Windows\System32\drivers\etc\hosts`, then run `ipconfig /flushdns`.
   - Quit and reopen Margin so it drops its open connection. Wait 30 minutes.
   - Expected: one "<account> · Usage not updating" with body "Can't reach the usage server · Last reading …". The Codex account keeps updating. No repeat in the next 30 minutes.
   - Undo on Mac: `sudo sed -i '' '/api.anthropic.com/d' /etc/hosts`, then flush as above. On Windows, remove both lines and flush.
   - Expected after undo: opening the popover updates the card within seconds, with no further alert. Block again for 30 minutes: one new alert.
   - Silent cases. Expected no alert for each: Wi-Fi off for 40 minutes; the machine asleep for 40 minutes; the same block with only Claude accounts in Margin (Margin can't tell that from being offline).

## Make active

5. **Claude switch and Undo.** Two dedicated Claude accounts, one in `~/.claude` and one signed in with `CLAUDE_CONFIG_DIR=~/.claude-test claude`. Settings → Accounts → **Make active** on the second, confirm.
   Expected: no Keychain prompt on Mac. The toast says it is active and the **Active** tag moves. `claude` then `/status` in the default folder shows the second account, and `CLAUDE_CONFIG_DIR=~/.claude-test claude` then `/status` shows the first. On Mac, Keychain Access lists `Margin backup` items.
   Press **Undo** in the toast. Expected: both folders return to their original accounts, with no prompt.
   On Windows, repeat with `claude` closed and with it open.
6. **Still signed in the next day.** After check 5, leave Margin and both CLIs alone overnight, then use `claude` in both folders.
   Expected: both still signed in, with neither account signed out in Margin.
7. **Codex switch.** Two dedicated Codex accounts, the second signed in with `CODEX_HOME=~/.codex-test codex login`. Quit Codex, including the desktop app and its background helper, then **Make active** and confirm.
   Expected: `codex` uses the new account, and so does the Codex app once reopened. While Codex is running, Make active refuses with a message. Undo restores both folders. Check the next day as in check 6.

## Windows

8. **Install.** Run `Margin_<version>_x64-setup.exe`, past SmartScreen with **More info → Run anyway**.
   Expected: Margin starts in the tray with no console window. Also install the MSI once.
9. **In-app update.** Install the previous release, then Settings → **Check for updates** → install.
   Expected: Margin restarts on the new version with accounts and settings kept.
10. **Toast.** A test notification on Windows 11, and on Windows 10 if available.
    Expected: the toast names Margin. Clicking it while on screen opens the popover.
11. **Display scaling.** At 125%, 150%, and 200% (Settings → System → Display), open the popover and Settings.
    Expected: sharp text, no clipping, anchored to the tray, also on a secondary monitor if there is one.
12. **Taskbar icon.** At 16px on light and dark taskbars, with the taskbar on two different edges.
    Expected: the icon is legible and its usage state can be read in each.
13. **Command line.** `%LOCALAPPDATA%\Margin\Margin.exe --line`, `--line --all`, and `--json | more` (MSI: `C:\Program Files\Margin\Margin.exe`).
    Expected: the same usage as the popover.

## Mac

14. **In-app update, menu bar, and sleep.** Update in the app from the previous release. Check the popover against the icon on a light and a dark menu bar. Sleep and wake.
    Expected: new version with settings kept, popover under the icon, and the tray updated within 10 seconds of waking.
15. **12-hour clock.** Turn on 12-hour time in System Settings → General → Date & Time, then restart Margin.
    Expected: notifications and the popover show times like "4:30 PM".
16. **Claude Code statusline.** Add `margin --line` to Claude Code's statusline as in the README.
    Expected: it matches the popover, and shows the account the terminal uses.

## Both platforms, over time

17. **Autostart after a reboot.** Turn on **Launch at login** and reboot.
    Expected: Margin is in the tray after login, once, and refreshes without being opened. Turn it off and reboot: Margin does not start.
18. **Idle overnight, Claude.** Leave a Claude account unused overnight with Margin running, and don't run `claude`.
    Expected in the morning: the card shows either a current reading or a labeled signed-out state with its age, never a fresh-looking old number. It recovers after running `claude` and refreshing, without a "Usage not updating" notification for the expired token.
19. **Weekly digest.** Turn on **Weekly digest**.
    Expected: one notification the next Monday at or after 09:00, not two.
20. **Uninstall.** Follow [install.md](install.md).
    Expected: Margin exits cleanly and CLI sign-ins still work.

## Results

Copy a row for each run. Link an issue for every failure.

| Date | Version | Platform and OS version | Checks | Pass or fail | Notes |
| --- | --- | --- | --- | --- | --- |
|  |  |  |  |  |  |
