# Handoff: TAW Player (PL-001)

Written 2026-10-14 when the family's work moved from one shared chat to a chat per app. Read CLAUDE.md, docs/ARCHITECTURE.md and ROADMAP.md, then this.

## Where it stands

- The repo has no `main` yet: everything is on branch `claude/focused-ritchie-veg4m9` (first build, 2026-10-13), version 0.1.0 Unreleased. CI is green (Linux and Windows builds as artifacts).
- Works: folders, files and M3U/M3U8; FLAC, MP3, WAV, AIFF, ALAC/AAC M4A, Ogg Vorbis; gapless (measured seamless with sine test tracks); native rate where the output allows it, honest "Resampled to …" when not; tags and artwork; meters and spectrum; hand-off from a second launch (the Library's Play key); family keys.
- Still the **Jupiter look**. The family is moving to the TAW look (TAW-Panel `hifi`; the Analyser and Equaliser branches show how) with Spool Light Amber as the default; the reviewed mock-up is TAW-Panel `docs/finishes-review.html` (Separates, Now Playing and Mini layouts; Simple and Detailed).
- Not yet: WASAPI exclusive mode, AAC gapless trimming, media keys, VU meters, playlist editing, CUE sheets.

## Next

1. The user's Windows listening test of the first build.
2. Move to the TAW look (Separates layout first), with the window buttons and Settings drawer as in the Analyser.
3. Exclusive mode and media keys.
