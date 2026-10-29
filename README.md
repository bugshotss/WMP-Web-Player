# WebSkin Player

Original Windows Media Player skins (`.wmz`) and WMP-style visualizations for whatever your browser is playing:
SoundCloud or Spotify in Chrome, Edge, or any app that shows up in the Windows media controls.

Microsoft is ending skin support in Windows Media Player Legacy on November 10, 2026
([notice](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/wmp/effects-windowed)).
WebSkin Player re-implements the skin engine, so those skins keep working, with your web music instead of local files.

## Install

1. Download **`WebSkinPlayer-<version>-Setup.exe`** from the [latest release](../../releases/latest).
2. Run it. It installs for your Windows account only, needs no admin rights and no separate .NET install, and adds a
   **WebSkin Player** Start menu shortcut.
3. Start playing something in the browser. The player picks it up automatically.

Notes:
- **Windows SmartScreen** may say it "protected your PC", because the installer isn't code-signed. Choose
  **More info → Run anyway**.
- **Opening `.wmz` files:** Windows asks which app to use the first time you open one. Pick **WebSkin Player** and
  tick **Always**.
- **Updates:** the app checks this repository's releases when it starts and shows **Update available** in full mode
  (and in a skin's right-click menu). Running the new Setup.exe upgrades in place and keeps your skins and settings.
- **Uninstall** from Settings → Apps. It asks whether to delete your downloaded skins and settings too.
- **Portable:** the release also has a `portable.zip`. Unzip it anywhere and run `WebSkinPlayer.exe`. Put an empty
  file named `portable` next to the exe to keep settings and skins in a `Data` folder beside it.

Requires 64-bit Windows 10 (version 2004 or later) or Windows 11.

## Getting skins

No skins are bundled. They belong to their original authors.

- **Skin Chooser → Get more skins** lists the 175-skin
  [Windows Media Player Skins Collection](https://archive.org/details/windowsmediaplayerskinscollection) on archive.org,
  with a screenshot of each skin taken in WMP. Download one (**Download and Apply**) or all of them (about 300 MB).
  On a new install the app opens there.
- **Install skin file…** accepts `.wmz`, `.wms` and `.zip` (every `.wmz` inside a zip is installed). Opening a `.wmz`
  from Explorer installs it and switches to it.
- [WMP Goodies](https://sites.google.com/view/wmpgoodies/skins) lists another 266 skins (about 150 not in the
  archive.org collection). Download them from there and use **Install skin file…**.

The Skin Chooser groups skins by **publisher**: the company a skin was made for (Microsoft, Disney, Warner Bros.,
Alienware...), with skins by individual artists under *Independent artists*. Pick one publisher from the list under
the search box, or type a publisher or designer name into the search. A skin's details show who designed it, e.g.
*Disney · designed by The Skins Factory*. Publishers come from each skin's own copyright line, with corrections in
`src\WebSkinPlayer\Skins\SkinCatalog.json`.

## Modes (as in WMP)

| Mode | How | What |
|---|---|---|
| Full mode | Ctrl+1 | Now Playing with a large visualizer, plus the **Skin Chooser** (live previews, *Apply Skin*). |
| Skin mode | Ctrl+2 | The selected `.wmz` skin, fully interactive. Right-click a skin for the app menu. |
| Taskbar widget | Ctrl+3 | Docks in the Windows taskbar: art, title/artist, live mini-spectrum, previous/play/next. Click to open the player, scroll for volume, drag to move it, right-click for options. *Keep widget on the taskbar* shows it in every mode. |

Other shortcuts: Ctrl+P play/pause, Ctrl+S stop, Ctrl+B / Ctrl+F previous / next, F8 mute, F9 / F10 volume.
In full mode, double-click the visualizer (or press Alt+Enter) for full screen. Esc exits full screen.

The player follows whatever is playing. To pin one app instead, use **Source** in full mode.

**Always on Top** keeps the player above other windows, in full mode and in every skin window. Switch it with the
pin button in full mode's transport bar, or from the right-click menu of a skin, the visualizer, or the taskbar widget.
Switch it off to let other windows cover the player again. The setting is remembered.

## The skin engine

It re-implements WMP's skin format:
- **Elements:** views, subviews, buttons and button groups (mapping images), sliders, custom sliders, text,
  effects, video, playlist, popups, equalizer and video settings.
- **Scripting:** `jscript:` expressions, `wmpprop:` / `wmpenabled:` bindings, `*_onchange` and player events, all
  running the skins' own JScript.
- **Resources:** `res://wmploc.dll/...` strings, images and scripts come from the PC's Windows Media Player.

It also reproduces the WMP quirks that skins depend on:
- Names in scripts are case-insensitive.
- A property can be read with call syntax, e.g. `mediacenter.effectType()`.
- When `scriptFile` is omitted, the `.js` named like the `.wms` loads by default.
- A VIEW's `clippingColor` and `transparencyColor` key the whole composed window.
- A subview's `clippingColor` (with no `clippingImage`) cuts its background image into the subview's shape, which
  also clips its children.
- `onmouseout` fires only when the pointer leaves an element, not when it moves onto one of the element's children
  (Revert fades its window on the view's `onmouseout`).
- Setting `theme.currentViewID` switches to another view.
- Preview views run with no `player` object, so the Skin Chooser shows them as WMP did.
- `WMPImage_AlbumArt*` shows the current track's artwork.
- Damaged zip headers are tolerated.
- Animated GIFs play with their own frame timing, disposal, and loop count. Intro animations and shutters that play
  once and end transparent (Kenwood, Blinx, STALKER) uncover what's underneath.
- Visualizer names from WMP 7/XP (Spikes, Ambience, Musical Colors, Plenoptic, Particle) map to the closest current
  effect, and the skin still reads back the name it set, so its own mode logic works.

Many skins show the visualizer only on demand, exactly as in WMP: Kenwood defaults to track info (press
**Previous Preset**, the top arrow in the right-hand cluster), Mecha and Plasma Ball have a visualizer button, and
Gold slides it out in a drawer.

**Compatibility across the collection:** all 175 skins load. 155 run their scripts without any error. The other 20
hit bugs in their own scripts, such as references to elements the skin doesn't define. WMP silently ignored those
errors, and so does this app; the errors are written to the log only.

**Windows Media Player Legacy** (Settings → System → Optional features) supplies WMP's built-in button images and
strings that some skins borrow, and the Alchemy visualizer. Without it, skins still load but those borrowed pieces
are blank and Alchemy isn't listed.

## Screen saver

**Screen Saver** (in the right-click menu of the visualizer, a skin, or the taskbar widget) turns on the visualizer
screen saver. Once the PC has been idle for the chosen time (1 to 30 minutes), every monitor shows a random
visualization, switching to another one every 15 seconds to 5 minutes, with the current track fading in at the bottom.
Moving the mouse or pressing a key closes it. **Lock the PC When It Closes** locks Windows as it closes, like a
screen saver with "On resume, display logon screen". **Start Now** shows it straight away.

Like Windows' own screen saver it stays away while a full-screen game, presentation or video is running, and while
the PC is locked. Music playing in a browser tab doesn't hold it back. Windows doesn't let apps draw on the lock
screen itself, which is why this is a screen saver.

## Visualizations

- **Alchemy:** the real WMP plug-in from `mpvis.dll`, hosted and fed system audio.
- **Bars and Waves:** Bars, Ocean Mist, Fire Storm, and Scope. **Battery:** 8 presets. **Album Art.**
  WMP's own Bars and Battery are locked inside `wmp.dll`, so these are faithful recreations.
- Any other WMP visualization plug-in registered on the PC appears automatically.
- **Each skin remembers its visualization.** A skin opens with the one last shown in it; a skin you haven't used yet
  starts with its own (or WMP's default, Bars). Pick one from a skin's right-click Visualizations menu and the skin
  switches to it; pick one from the list in full mode (Ctrl+1) and it's the one the skin shows when you go back. Skins that name a visualization as they switch theirs on (Kenwood sets Spikes every time it
  leaves its song display) get the remembered one instead, once there is one. Song changes don't count as a pause and
  resume, even when the browser reports a moment of pause between songs, so skins don't drop back from their
  visualizer at every song. A plug-in that duplicates a built-in visualization (WMP 8's own Bars and Waves and
  Battery) isn't listed a second time.
- **Old 32-bit plug-ins** from the WMP 7 to 10 era (WMP 8's original Ambience, Particle, Plenoptic and Spikes,
  3D Alchemy, the Averett & Associates packs, the Psychedelia pack, community ones) run in a small 32-bit helper
  (`VisHost32`), because a 64-bit app can't load them. Add them with **Add Visualization…** in the visualizer's
  right-click menu: pick the plug-in's `.dll`, a `.zip` containing it, or its `.msi` installer (unpacked, not
  installed). Nothing is registered with Windows and no admin rights are needed; each one is test-rendered first and
  only added if it draws. Many are on [WMP Goodies](https://sites.google.com/view/wmpgoodies/visualizations); their
  older `.exe` installers are self-extracting archives that 7-Zip opens.

  The helper treats them the way WMP 9 did: WMP 9-style plug-ins get a player object and a (hidden) window before
  they draw, and a preset is always selected. DirectX ones that draw onto that window instead of the DC they're given
  are copied off it. It also runs with Windows' compatibility registry virtualization, as XP-era programs do, because
  some plug-ins open WMP's machine keys for writing just to read them. Still not working: Microsoft's WMP 9 and 10
  promotional visualizations and the ones built on the same engine (Grind, Terminator III, Looney Tunes), which
  present only black frames; PictureViz II, which waits on a folder dialog; Corona and Fruity, whose registration
  fails.

Audio for the visualizers is captured with WASAPI loopback, so they react to everything Windows plays.

## Limits (web players can't be driven like local files)

- Equalizer, balance and video settings are kept and shown in skins, but can't change the browser's audio.
- Volume and mute control the source app's own entry in the Windows volume mixer (e.g. chrome.exe).
- Seeking, shuffle and repeat work when the site supports them through the Windows media controls.
  Fast-forward and rewind seek ±10 seconds.
- There's no video. Skins with a video area but no visualizer area show the visualizer there.

## Files

- Settings: `%LOCALAPPDATA%\WebSkinPlayer\settings.json`
- Skins: `%LOCALAPPDATA%\WebSkinPlayer\Skins`
- Log: `%LOCALAPPDATA%\WebSkinPlayer\log.txt`
- The app itself: `%LOCALAPPDATA%\Programs\WebSkin Player`

## Building

Needs the .NET 10 SDK, and [Inno Setup 6](https://jrsoftware.org/isinfo.php) for the installer
(`winget install JRSoftware.InnoSetup`).

```
dotnet build src\WebSkinPlayer
.\tools\Build-Release.ps1
```

`Build-Release.ps1` writes `artifacts\WebSkinPlayer-<version>-Setup.exe` and the portable zip. Both are
self-contained, so the target PC needs no .NET install.

**Releasing:** set `<Version>` in `src\WebSkinPlayer\WebSkinPlayer.csproj`, then push a matching tag:

```
git tag v0.2.0
git push origin v0.2.0
```

The **Build** workflow (`.github/workflows/build.yml`) builds every push and pull request. For a `v*` tag it also
publishes a GitHub release with the installer and zip attached. Builds made by the workflow know their repository and
check it for updates; local builds don't.

### Developer hooks

- `WebSkinPlayer.exe --test-skins <folder|file> <outDir>` loads every skin headlessly. For each skin it writes
  `<skin>.png` (the Skin Chooser preview) and `<skin>.tree.txt` (the element tree), plus a `report.txt` that
  lists script errors.
- `WebSkinPlayer.exe --test-live <folder|file> <outDir> [seconds]` opens each skin for real but hidden and sandboxed.
  Scripts, timers, GIFs, and player events all run, but nothing reaches your player, volume, browser, or settings.
  Afterwards it measures how much of every visualizer area is actually visible, and for a covered one names what is
  drawn over it (`live-report.txt`).
- `WebSkinPlayer.exe --automate script.txt` replays commands through the real input code:
  `click x y`, `drag x1 y1 x2 y2`, `eval js`, `capture file.png`, `capturefull file.png`, `vis Bars 0`,
  `skin path`, `widget`, `restore`, `fullmode`, `tab skins|online`, `uiclick ButtonName`, `uiselect ListName text`,
  `state label`, `wait ms`, `exit`.
- `--debug-input` logs every press and what it hit. `--widget` starts collapsed into the taskbar widget.
  `--exit` closes a running copy (the installer uses it).
- `WEBSKINPLAYER_DATA=<folder>` runs with a separate data folder, e.g. to try a first run on a clean profile.
  `WEBSKINPLAYER_TESTSIGNAL=1` feeds made-up music to the visualizers.
- `WebSkinPlayer.exe --import-vis <dll|zip|msi|folder> [label]` adds visualization plug-ins and writes
  `Visualizations\import-report.txt`. Automation also has `saver start|stop|next|show <id>|?` and `capturesaver file.png`.
- `WebSkinPlayer.VisHost32.exe probe <dll>` describes a plug-in DLL; `snapshot <dll> <clsid> <out.bmp> [preset]
  [dc|attach|windowed]` test-renders one and reports how much appeared in the DC (`lit`) and on its window
  (`windowLit`). `WEBSKINPLAYER_VISHOST=<exe>` points the player at a development build of the helper.

### Layout (`src\WebSkinPlayer`)

- `Media\`: the Windows media session bridge, per-app volume, and the WMP-style player model.
- `Audio\`: loopback capture and FFT, producing WMP's `TimedLevel`-shaped frames.
- `Visualization\`: built-in effects, the hosted `IWMPEffects` adapter, the catalog, and the renderer.
- `Skins\`: the package/resource/parser layers, the object model, `Scripting\` (Jint bridge and JScript
  compatibility), `Elements\`, the views, windows and themes, and the archive.org skin downloader.
- `UI\`: the full-mode window and the taskbar widget.
- `src\WebSkinPlayer.VisHost32`: the 32-bit plug-in helper (shared-memory protocol in `Serve.cs`, mirrored by
  `Visualization\Hosted32Visualization.cs`).
- `installer\`: the Inno Setup script. `tools\`: the release build and icon scripts.
- `spikes\`: throwaway experiments. `SmtcTestSource` is a fake media session with a quiet tone, used for tests.

## Credits

Skins are the work of their original authors and aren't part of this repository. Uses
[Jint](https://github.com/sebastienros/jint) (BSD-2-Clause) for JScript and
[NAudio](https://github.com/naudio/NAudio) (MIT) for audio capture.
