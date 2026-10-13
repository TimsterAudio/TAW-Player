# Changelog

## Unreleased

The Player's first build (PL-001): the native successor to Spool.

- Plays folders (with sub-folders, in name order), files and M3U/M3U8 playlists, opened from the panel, dropped on the window, or given on the command line (`taw-player --play …`).
- FLAC, MP3, WAV, AIFF, ALAC and AAC (M4A) and Ogg Vorbis, decoded with symphonia.
- Gapless: tracks at the same rate play as one continuous stream; LAME-tagged MP3s are trimmed of encoder delay and padding.
- Each track at its own sample rate where the output allows it; otherwise resampled, and the panel says which.
- Play, pause, stop, next, previous, seek, random play, volume and output device.
- Tags and artwork (folder cover or embedded picture), LED meters and a spectrum.
- A second launch hands its files to the running Player (the Library's Play key uses this).
- Family keys start the Library, Equaliser and Analyser when they're installed, and say plainly when one isn't.
- Black or Silver panel; settings remembered.
