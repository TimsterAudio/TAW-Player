# Handoff: TAW Player (PL-001)

Written 2026-10-14 when the family's work moved from one shared chat to a chat per app. Read CLAUDE.md, docs/ARCHITECTURE.md and ROADMAP.md, then this.

## Where it stands

- **0.2.0 built on 2026-10-15** (branch `claude/kind-johnson-du69z8`), from the Linux Mint test of 0.1.0 (brief in `docs/briefs/2026-10-14-0.2.0.md`): opening music waits for Play unless Settings says otherwise (`--play` always plays); Add, and Shift while dropping, add to the end of the list, gaplessly if it's still playing; a dB scale under the meters (TAW-Panel 0.3.2's `meter_scale`); the meters centred with the spectrum off; media keys and the desktop's media controls (MPRIS checked with `playerctl` under PipeWire; SMTC compiles but is untested on Windows); back to start (Home); and the bottom-corner resize. Then, from two rounds of the user's feedback on the look: the nameplate is the badge and model line at the top left with no big name (TAW-Panel 0.4.0, for the whole hi-fi series), the top band is shorter, the list's keys are on the nameplate's row over the display, the list's size sits under the track's details, artwork grows up to 1000 × 1000 (the artwork column also leaves the list's keys room beside the family switcher), Simple shows only the artwork and the track's names, meter colours are a setting (Classic or Display), and the settings drawer has separated sections and a left-aligned Keys list. The corner resize was in TAW-Panel (`resize_edges`): the corners lost to the edges in egui's hit test, and after any window drag egui kept the old drag because the window manager eats the button release, so every other resize did nothing. Fixed in TAW-Panel 0.3.2 (commit `5290f15`, branch `claude/kind-johnson-du69z8` there: merge it with a merge commit, not a squash, so the pin stays valid). The Library, Equaliser and Analyser have the same resize bug until they move to 0.3.2 or later. The Player pins TAW-Panel 0.4.0 (`d030ffd`); the panel's planned resizable list is now its 0.5.0.

- **0.1.0 released on 2026-10-14**: `main` was created from branch `claude/focused-ritchie-veg4m9`, and release.yml publishes `v0.1.0` with the Linux and Windows downloads.
- Works: folders, files and M3U/M3U8; FLAC, MP3, WAV, AIFF, ALAC/AAC M4A, Ogg Vorbis; gapless (measured seamless with sine test tracks); native rate where the output allows it, honest "Resampled to …" when not; tags and artwork; meters and spectrum; hand-off from a second launch (the Library's Play key); family switcher.
- **The TAW look** (2026-10-14), on TAW-Panel 0.3.0 like the Equaliser and Analyser: the Separates layout from TAW-Panel `docs/finishes-review.html`. Nameplate, family switcher, Simple/Detailed and the Settings drawer as in the Analyser; artwork and the track on the left; the display (time, meters, spectrum) and the list on the right; the transport bar along the bottom. Checked under Xvfb with playback in Spool and Porcelain, light and dark, Simple and Detailed, at 1280 × 800, 1280 × 720, 1600 × 900 and 960 × 640.
- Fixed on the way: jumping to a track or seeking no longer counts as a dropout (the engine stops counting while it empties the ring on purpose).
- Now Playing and Mini layouts from the mock-up aren't built yet.
- Not yet: WASAPI exclusive mode, AAC gapless trimming, VU meters, playlist editing (beyond adding), CUE sheets.

## Next

1. The user's test of 0.2.0, on Windows and Linux: media keys (SMTC on Windows is untested), Add while the last track plays, the corners, MP3 gapless and opening the Library's M3U8.
2. 0.3.0: WASAPI exclusive mode (see ROADMAP.md's pipeline); then Now Playing and Mini layouts.

