# Roadmap

The Player is the native successor to Spool. This is where it stands against Spool 1.5.3, and what comes next. Ticked items are in the Player now.

## Spool parity

### Playback
- [x] Gapless playback (one continuous stream; FLAC/WAV/AIFF/ALAC sample-accurate)
- [x] MP3 gapless trimming from the LAME/Xing header
- [ ] AAC (M4A) gapless trimming from iTunSMPB (symphonia doesn't read it yet)
- [x] Each file at its native rate; honest display of what happens to it (Spool: rate badge)
- [x] Seek, previous/next, play/pause
- [x] Random play (shuffle the list and play it)
- [ ] Jump to the start of the list (Spool's back-to-start key)
- [x] Space, Shift+→/← shortcuts
- [ ] Hardware media keys and system media controls (MPRIS on Linux, SMTC on Windows), with title and artwork
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
- [ ] Spool's meter scale (20 segments, one per dB at the top) and its four peak LEDs with hold (0.5–10 s or until cleared)
- [ ] VU meters (needle, true VU ballistics, 0 VU reference −20 to −6 dBFS), in their own window
- [x] Spectrum (new; from TAW-Panel)

### Looks and options
- [x] Black / Silver panel (the family's version of Dark/Light)
- [ ] Retro (cassette deck) look, tape counter: waits for the skins work (TAW-Panel ROADMAP)
- [x] Include sub-folders (setting kept; key to come)
- [ ] Options panel: sub-folders, meter hold, VU reference
- [ ] Activity log (opt-in, local JSON Lines file)

### Deliberately the same as Spool
- Never writes to, renames or moves your files.
- No network, no accounts, no telemetry.

## Beyond Spool

- [ ] WASAPI exclusive mode on Windows, so the mixer is bypassed and the device follows each track's rate
- [ ] Gapless across a rate change where the device can switch without a pause
- [ ] CUE sheets (one file, many tracks)
- [ ] Playlist editing: add, remove, reorder; save as M3U8
- [ ] Browse the Library's database (read-only) from the Player
- [ ] Remember the list and position between runs (optional)
- [ ] ReplayGain (off by default; it changes the samples, so the panel would say so)
- [ ] Multichannel downmix to stereo
- [ ] Panel skins (TAW-Panel ROADMAP)
