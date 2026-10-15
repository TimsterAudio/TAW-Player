# Roadmap

The Player is the native successor to Spool. This is where it stands against Spool 1.5.3, and what comes next. Ticked items are in the Player now.

## Pipeline (proposed 2026-10-14)

Where the items below land, release by release. Sizes: S = one session, M = two or three, L = a release of its own. Briefs from the family session are in `docs/briefs/`; TAW-Panel's `docs/TRACKER.md` tracks the family as a whole.

| Release | What | Needs | Size |
|---|---|---|---|
| 0.2.0 ✓ | **Released 2026-10-15.** Feedback from the 0.1.0 test on Linux Mint** ([brief](docs/briefs/2026-10-14-0.2.0.md)): don't start playing on open (a setting, off by default; `--play` still plays); an Add key to append to the list; a dB scale on the meters; meters centred when the spectrum is off; media keys and system media controls (MPRIS, SMTC); the bottom-corner resize; the back-to-start key | TAW-Panel 0.4.0 (meter scale; the resize fix; the hi-fi look tightened) | M |
| 0.3.0 | WASAPI exclusive mode; gapless across a rate change where the device allows | A real Windows test | L |
| 0.4.0 | Now Playing and Mini layouts; Spool's meter scale and peak hold; the VU window; resizable list columns | TAW-Panel 0.5.0 and 0.6.0 | M |
| 0.5.0 | Playlist editing (reorder, remove, save as M3U8); remember the list and position; CUE sheets | — | M–L |
| later | Browse the Library's database; ReplayGain; downmix; Opus; AAC gapless when symphonia reads iTunSMPB; the retro deck look with skins | — | — |

Still to test on a real desktop: MP3 gapless, opening an M3U8 (the Library's Play), everything on Windows (SMTC media keys among it).

## Spool parity

### Playback
- [x] Gapless playback (one continuous stream; FLAC/WAV/AIFF/ALAC sample-accurate)
- [x] MP3 gapless trimming from the LAME/Xing header
- [ ] AAC (M4A) gapless trimming from iTunSMPB (symphonia doesn't read it yet)
- [x] Each file at its native rate; honest display of what happens to it (Spool: rate badge)
- [x] Seek, previous/next, play/pause
- [x] Random play (shuffle the list and play it)
- [x] Jump to the start of the list (Spool's back-to-start key; Home)
- [x] Space, Shift+→/← shortcuts
- [x] Hardware media keys and system media controls (MPRIS on Linux, SMTC on Windows), with title and artwork
- [x] FLAC, MP3, WAV, AAC; ALAC, AIFF and OGG everywhere (Spool depended on the browser)
- [ ] Opus

### Track info and artwork
- [x] Title/artist/album from tags, falling back to the file name
- [x] Tags for WAV, AIFF and OGG too (Spool couldn't)
- [x] Format and sample rate/bit depth badges
- [x] Folder cover (cover/front/album, walking up to the opened folder), then embedded art
- [x] The playlist follows the playing track, except just after you scroll it

### Meters
- [x] LED peak meters, instant rise, steady fall, at rest on pause
- [x] A dB scale on the LED meters (0, −3, −6, −12, −24, −48)
- [x] Meter colours: classic, or the display's own
- [ ] Spool's meter scale (20 segments, one per dB at the top) and its four peak LEDs with hold (0.5–10 s or until cleared)
- [ ] VU meters (needle, true VU ballistics, 0 VU reference −20 to −6 dBFS), in their own window
- [x] Spectrum (new; from TAW-Panel)

### Looks and options
- [x] Black / Silver panel (the family's version of Dark/Light), replaced by the TAW look
- [x] The TAW look: Spool or Porcelain, Light, Dark or Automatic, Spool's colour themes; Simple and Detailed; own title strip and window buttons
- [ ] Retro (cassette deck) look, tape counter: waits for the skins work (TAW-Panel ROADMAP)
- [x] Include sub-folders (setting kept; key to come)
- [ ] Options panel: sub-folders, meter hold, VU reference (Settings drawer has sub-folders and spectrum)
- [ ] Activity log (opt-in, local JSON Lines file)

### Deliberately the same as Spool
- Never writes to, renames or moves your files.
- No network, no accounts, no telemetry.

## Beyond Spool

- [ ] WASAPI exclusive mode on Windows, so the mixer is bypassed and the device follows each track's rate
- [ ] Gapless across a rate change where the device can switch without a pause
- [ ] CUE sheets (one file, many tracks)
- [ ] Playlist editing: remove, reorder; save as M3U8 (adding to the end is in: the Add key, or Shift while dropping)
- [ ] Browse the Library's database (read-only) from the Player
- [ ] Remember the list and position between runs (optional)
- [ ] ReplayGain (off by default; it changes the samples, so the panel would say so)
- [ ] Multichannel downmix to stereo
- [ ] Panel skins (TAW-Panel ROADMAP)
