# Design

The Player's panel is the TAW hi-fi look shared with the Library, Equaliser and Analyser: TAW-Panel's `hifi` module. Spool's warm panels are the default (Light, Amber), tinted by any of Spool's seven colour themes; Porcelain is the alternative; each in Light, Dark or Automatic. The maker's badge ("Timster's Audio Works - Player"), the slanted logo (PLAYER) and the model line (PL-001 · MUSIC PLAYER · V…) stay from the original panel. The reviewed mock-up is TAW-Panel's `docs/finishes-review.html` (the Separates layout).

## Layout

- **Nameplate (top):** badge, logo and model line on the left; the window buttons at the top right; under them the family switcher (Library, Player, Equaliser, Analyser; apps not found are dimmed), Simple / Detailed and the gear for Settings. Drag the strip to move the window; double-click it to maximise.
- **Left column:** the artwork (as big as the height allows), the title, artist and album, and what's playing:
  - Simple: a light and plain words (*Playing as recorded · CD quality*; *converted for this output*; *volume lowered*).
  - Detailed: the format chips (codec; bits, rate, channels), the signal path (*Native rate, untouched*, *Native rate · volume −6.0 dB*, *Resampled to 48 kHz for this output*), the output (audio system, device, rate, channels, sample format) and any dropouts.
  - Green means the samples reach the output at their own rate; amber means they're resampled. Never "bit-perfect".
- **Display window (Detailed only):** the time, big (click for remaining), with ELAPSED · TRACK nn under it; L and R LED meters; a spectrum under them while playing (Settings can switch it off).
- **List:** Open folder, Open files and Clear, with the track count and length (or the latest message) on the right; then the playlist: number (a play triangle on the playing track), title, artist, album, time and type (Simple drops album and type).
- **Transport bar (bottom):** Random, Previous, Play/Pause (large, in the accent), Next and Stop; the position bar with elapsed and total time; the volume. All on one line, centred.

## Settings (the gear)

Look (finish, light or dark, colour), Output (the device, and what it's running at), Playing (include sub-folders, spectrum), Window (use the system title bar) and the keys.
