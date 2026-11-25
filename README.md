# NightSlide

A small Windows tray app for changing the built-in Night Light strength. Click its moon icon in the notification area to open the slider above the icon. Right-click the icon to open or exit. Launching it again opens the existing widget without creating another tray instance.

## Build

Requires Windows 11, Rust 1.95 or newer, and the Windows SDK resource compiler (`rc.exe`) to embed the supplied icon. From this directory:

```powershell
cargo build --release --offline
./target/release/nightslide.exe
```

The app uses the cached `windows-sys` and `thiserror` crates. If your machine has not cached them, omit `--offline` to let Cargo download them.
Exit a running NightSlide instance before rebuilding the same executable, since Windows locks a running `.exe`.

To run the tests and window lifecycle check:

```powershell
cargo test --offline
powershell -ExecutionPolicy Bypass -File scripts/smoke.ps1 -Tray
```

The optional read-only live settings check is `cargo test --offline --test cloudstore reads_live_night_light -- --ignored`.

## Controls

The single-row widget has a sun at the 0% end and a crescent moon at the 100% end. Drag or click its square slider to set strength from 0% (6500 K) to 100% (1200 K). Any slider action turns Night Light on when it is off; there is no off control. Left and Right (also Down and Up) adjust strength by 1%. Ctrl with an arrow adjusts by 0.1%; Shift with Left or Right snaps to 0% or 100%. Home and End also jump to the limits. The widget accepts Tab without drawing a focus outline; Space and Enter activate Night Light at the current strength. Escape or switching to another window closes the widget. The hand pointer appears over the slider and changes to a grabbing hand while dragging; the rest of the widget uses the regular arrow cursor.

The supplied [white crescent icon](assets/white_crescent_moon.ico) is embedded in the executable and used in the tray.

## Compatibility

Windows does not provide a public API for Night Light strength. NightSlide includes a modified copy of [win-nightlight-lib 0.5.0](https://github.com/kvnxiao/win-nightlight-cli), which reads and writes the current user's Night Light CloudStore values; its MIT license is in [vendor/win-nightlight-lib/LICENSE](vendor/win-nightlight-lib/LICENSE). The upstream library has been tested on Windows 11 24H2 and build 26300. The binary format could change in later Windows releases. NightSlide shows a tray notification when a setting is unavailable. Only the library's registry adapter was changed to use the cached `windows-sys` crate.
