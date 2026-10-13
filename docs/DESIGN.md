# Design

The Player's panel follows Drone Box's design language, like the rest of the family: a Jupiter-style hardware panel, Black or Silver, the maker's badge ("Timster's Audio Works - Player"), the slanted logo (PLAYER) and the model line (PL-001 · MUSIC PLAYER · V…), the red/orange/yellow/cream stripe, coloured keys with LEDs, and red displays. The pieces come from TAW-Panel; the Equaliser's `docs/DESIGN.md` describes them in full.

## Layout

- **Top strip:** badge, logo and model line; the display (track and time; artist, album and format); **Transport** (Prev, Play, Pause, Stop, Next, Random), **Open** (Folder, Files, Clear) and **Family** (Library, EQ, Analyser).
- **Left column:** the artwork, the track's title, artist and album, the format chips and the signal-path line (green: native rate; amber: resampled), the position bar, the LED meters, and the output row (Volume, Output device, Spectrum, Black/Silver).
- **Right:** the spectrum strip (optional) and the playlist: number (a play triangle on the playing track), title, artist, album, time and type.
- **Footer:** messages on the left; the output (audio system, device, rate, channels, sample format, any dropouts) on the right.

## Keys

- Play's LED is lit while playing, Pause's while paused. Play stays latched while playing (pressing it again does nothing, as on a deck, and as in Spool's Retro look).
- Family keys are red with an LED that's lit when that app was found.
- Cream keys open things; grey keys move or stop; orange is Random.
