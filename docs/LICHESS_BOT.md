# Upgrade lichess-bot to Deep Lantern 0.2.0

1. Stop the bot after its current games finish.
2. Build this version with `cargo build --release` on the target computer.
3. Replace the executable referenced by your existing `engine.dir` and
   `engine.name`. macOS uses `deep-lantern`; Windows uses `deep-lantern.exe`.
4. Merge these fields into the existing **engine** section of `config.yml`.
   Keep the existing token, engine paths, matchmaking and challenge settings.
   Do not create a second engine/uci_options section.

```yaml
engine:
  ponder: true
  polyglot:
    enabled: false  # Deep Lantern owns the book in this configuration.
  uci_options:
    OwnBook: true
    BookFile: "/absolute/path/Cerebellum_Light_Poly.bin"
    BookMaxPly: 64  # Half-moves: up to 32 moves per side.
    # Retain your existing supported Hash / Threads / Move Overhead settings.
```

For Windows, use a quoted path such as `"C:/Chess/books/Cerebellum_Light_Poly.bin"`.
Use the real full path. Do not add `Ponder` under `uci_options`; python-chess
manages that option from `engine.ponder`. Remove unsupported engine options
left over from another engine (for example SyzygyPath or UCI_ShowWDL).
Restart the bot and verify `Deep Lantern 0.2.0`, `book loaded` and
`Polyglot book hit` in its engine logs (enable engine.debug if needed).
A load failure is reported and normal search remains available.

To enable only pondering, set `ponder: true` and leave `OwnBook: false`.
The engine cannot load compressed `.7z`/`.zip` books: extract the `.bin` first.

## Alternative: let lichess-bot own the book

The bridge already has its own Polyglot reader. If you prefer it, set
`uci_options.OwnBook: false`, `polyglot.enabled: true`, and use the existing
bridge's `polyglot.book.standard` list and `selection: "best_move"` setting.
Do not enable both readers. The engine-owned configuration above also works
in other UCI chess GUIs.

## Book selection and strength

The official Cerebellum page lists Cerebellum 3 Merge dated September 16, 2020:
https://zipproth.de/Brainfish/download/

This is a compatible format target, not a claim that it is the strongest book
available today. Its download returned HTTP 403 in the build environment, so
that exact file was not tested or bundled. Use the publisher's distribution
and terms. The selected highest-weight move follows the author's preference;
it does not independently establish the objectively strongest move.

The compatibility suite can verify any supplied book against python-chess:

```sh
python scripts/validate_features.py target/release/deep-lantern --book /path/book.bin
```

Book selection and pondering should be measured at your actual time control.
For fair engine-strength comparisons, use paired openings and equal pondering
settings, CPU allocation and clocks. No Elo gain has been measured for this
release. Pondering increases CPU use during the opponent's turn; concurrent bot
games share those resources.

Config reference checked for this update:
https://github.com/lichess-bot-devs/lichess-bot/blob/master/config.yml.default
