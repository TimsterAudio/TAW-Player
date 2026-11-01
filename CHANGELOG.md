# Changelog

## Unreleased

## 0.2.2 – 2026-11-01

- **The warm finish is called Linen** (it was Spool) in Settings. It looks the same, and a saved "Spool" choice opens as Linen. From TAW-Panel 0.5.0.

## 0.2.1 – 2026-11-01

- **One Player at a time, from anywhere.** Clicking the Player in another app's family switcher (or opening the Player again) brings the open one to the front, restored if it was minimised, instead of starting a second Player. Files handed over (the Library's Play key) now reach a minimised Player too.
- Starting another family app no longer leaves a finished process behind each time.

## 0.2.0 – 2026-10-15

From the 0.1.0 test on Linux Mint.

- Opening music no longer starts it playing: opening a folder or files, dropping them on the window, or giving them on the command line fills the list and waits for Play. Settings has "Start playing when I open music" (off) to have it play straight away. `taw-player --play …` (the Library's Play key) always plays at once.
- **Add** next to Open folder and Open files: adds files, a playlist or a folder to the end of the list instead of replacing it. Holding Shift while dropping does the same. If the list is still playing, it plays on into what was added without a gap.
- A dB scale under the meters (0, −3, −6, −12, −24, −48), so a top end that lights on loud masters reads as what it is: peaks within 3 dB of full scale.
- With the spectrum off, the meters sit in the middle of the display.
- Media keys and the desktop's media controls: play, pause, next, previous, stop and seeking from the keyboard's media keys, the sound menu (MPRIS on Linux, `playerctl` too) and Windows' media overlay, which show the track's title, artist, album, length and artwork.
- Back to start (|◀◀ next to Previous, or Home): back to the first track of the list, as in Spool.
- A tidier front panel (TAW-Panel 0.4.0): the "Timster's Audio Works - Player" badge at the top left with the model line under it, and no big PLAYER name; a shorter top band; Open folder, Open files, Add and Clear on the nameplate's row over the display, in line with the family keys; the display and the list moved up; the track count and total time (and messages) under the track's details; artwork that grows with the window up to 1000 × 1000; one, slightly tighter corner radius throughout.
- Simple shows the artwork and the track's names and nothing else; Detailed adds the format, the signal path, the output and the list's size.
- Meter colours in Settings: Classic (green, yellow, red) or Display (the display's own colour, the top 3 dB still red).
- Settings: the chosen option in each control stands out a little more; the sections (Look, Output, Playing, Display, Window, Keys) are set apart with a line between them; the Keys list is aligned to the left.
- The window resizes from all four corners, and every time; before, a corner moved only one side, and every other resize or move did nothing (TAW-Panel 0.3.2).

## 0.1.0 – 2026-10-14

The Player's first release (PL-001): the native successor to Spool, in the family's TAW look.

- Plays folders (with sub-folders, in name order), files and M3U/M3U8 playlists, opened from the panel, dropped on the window, or given on the command line (`taw-player --play …`).
- FLAC, MP3, WAV, AIFF, ALAC and AAC (M4A) and Ogg Vorbis, decoded with symphonia.
- Gapless: tracks at the same rate play as one continuous stream; LAME-tagged MP3s are trimmed of encoder delay and padding.
- Each track at its own sample rate where the output allows it; otherwise resampled, and the panel says which.
- Play, pause, stop, next, previous, seek, random play, volume and output device.
- Tags and artwork (folder cover or embedded picture), LED meters and a spectrum.
- A second launch hands its files to the running Player (the Library's Play key uses this).
- The family switcher starts the Library, Equaliser and Analyser when they're installed, and says plainly when one isn't.
- The TAW look (TAW-Panel 0.3.0), as the Equaliser and Analyser: Spool's look by default (Light, Amber) with Spool's seven colour themes, or Porcelain; Light, Dark or Automatic. Artwork and the track on the left, the display (time, meters, spectrum) and the list on the right, and the transport along the bottom.
- Simple view: the artwork, the track, the list and the transport, with what's playing in plain words. Detailed adds the display and the exact format, signal path and output.
- The Player draws its own title strip and window buttons; Settings has "Use the system title bar", the output device, sub-folders and the spectrum.
- Settings remembered.
