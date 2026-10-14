# Handoff: TAW Player (PL-001)

Written 2026-10-14 when the family's work moved from one shared chat to a chat per app. Read CLAUDE.md, docs/ARCHITECTURE.md and ROADMAP.md, then this.

## Where it stands

- The repo has no `main` yet: everything is on branch `claude/focused-ritchie-veg4m9` (first build, 2026-10-13), version 0.1.0 Unreleased. CI is green (Linux and Windows builds as artifacts).
- Works: folders, files and M3U/M3U8; FLAC, MP3, WAV, AIFF, ALAC/AAC M4A, Ogg Vorbis; gapless (measured seamless with sine test tracks); native rate where the output allows it, honest "Resampled to …" when not; tags and artwork; meters and spectrum; hand-off from a second launch (the Library's Play key); family switcher.
- **The TAW look** (2026-10-14), on TAW-Panel 0.3.0 like the Equaliser and Analyser: the Separates layout from TAW-Panel `docs/finishes-review.html`. Nameplate, family switcher, Simple/Detailed and the Settings drawer as in the Analyser; artwork and the track on the left; the display (time, meters, spectrum) and the list on the right; the transport bar along the bottom. Checked under Xvfb with playback in Spool and Porcelain, light and dark, Simple and Detailed, at 1280 × 800, 1280 × 720, 1600 × 900 and 960 × 640.
- Fixed on the way: jumping to a track or seeking no longer counts as a dropout (the engine stops counting while it empties the ring on purpose).
- Now Playing and Mini layouts from the mock-up aren't built yet.
- Not yet: WASAPI exclusive mode, AAC gapless trimming, media keys, VU meters, playlist editing, CUE sheets.

## Next

1. The user's test of the first build in the new look, on Windows and Linux (listening for gaps; the signal-path line; the look).
2. A release (0.1.0): move Unreleased into a version section and merge; this repo has no `main` yet, so the first merge creates it.
3. Exclusive mode and media keys; then Now Playing and Mini layouts.

