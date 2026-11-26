# PlayStation button prompts

The Xbox build ships only Xbox button art. `victory glyphs` builds PlayStation
prompts from your own copy of the game, and VIctory serves them at read time:
no game file on disk is changed.

```
victory glyphs [--game <game folder>] [--xelu <folder>] [--out <folder>]
set VICTORY_FILE_PATCH=<the patches.txt it printed>
```

- **Prompts in the HUD and menus:** the face-button letters become cross,
  circle, square and triangle, LB/LT/RB/RT become L1/L2/R1/R2, and View
  becomes the touchpad. The symbols are Kenney's
  [Input Prompts](https://kenney.nl/assets/input-prompts) (CC0, built in),
  drawn inside the game's own button frames. Menu (the same icon as Options),
  the sticks and the D-pad stay as they are.
- **Settings controller picture (optional):** pass `--xelu` with the `PS5`
  folder of Xelu's
  [Free Controller & Key Prompts](https://thoseawesomeguys.com/prompts/)
  (CC0; download it yourself). The DualSense line art becomes a white
  controller, without the PS logo.

## How it works

- `VICTORY_FILE_PATCH` (`crates/victory-hle/src/modules/dstorage.rs`,
  `file_patch`) names byte ranges of the game's files to serve from other
  files. Each line is `<file name> <offset> <blake3 of the original range>
  <new bytes>`, and a range is patched only when the file there still hashes
  as listed, so another game version is left alone.
- The prompt icons are one zlib entry in `data.rpf`. The archive's table is
  encrypted and not touched, so the rebuilt movie keeps the old one's length
  (zeros after its `End` tag) and its stream the old stream's exact length
  (zopfli, then stored blocks to fill; `victory_formats::zlib_exact`). Only
  the label shapes change (`victory_formats::swf`,
  `victory_game::prompt_glyphs`); the game reads exactly the stream's length.
- The Settings picture is a 1024x512 BC7 texture, tiled 64 KiB `D`, in one
  zlib stream. The menu draws it at about 0.75 of its width, so the picture
  is drawn wider (`victory_game::settings_diagram`, our mode-6 BC7 encoder
  `victory_formats::bc7`).
- Everything it writes is derived from your copy: it goes to the cache folder
  (default `<cache>/file-patches/playstation`), never into the repository.

Checked on Linux under Wine (`docs/linux-wine.md`): prompts and the Settings
picture showed in game (runs 52-55, made by a Python prototype of the same
steps). The output of `victory glyphs` renders the same as that build: the
prompt icons pixel for pixel, the picture within rounding (60 dB).

Credits: prompt symbols by Kenney (kenney.nl), controller picture by Nicolae
"Xelu" Berbece and Paul Paun; both CC0. PlayStation and DualSense are
trademarks of Sony Interactive Entertainment.
