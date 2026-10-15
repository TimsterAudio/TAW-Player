# Architecture

The Player is one program with three kinds of thread: the **panel** (egui), the **player thread** (decoding, resampling, the output device), and the **output callback** (cpal's audio thread). Small worker threads read tags and artwork.

```
panel ──Cmd──► player thread ──samples──► ring buffer ──► output callback ──► device
  ▲               │  (symphonia, rubato,                  (only atomics and
  │               │   cpal device setup)                   the ring)
  └── Status ◄────┘
  └── atomics (played frames, peaks, pause, volume) ◄──── output callback
```

## The player thread (`engine.rs`)

It owns the queue (the list of paths), the open output and the track being decoded, and keeps the ring buffer about a second and a half ahead of the output.

- **Gapless.** When a track's decoder runs out, the next track is opened at once and its samples follow the last ones into the same ring. Nothing between them is inserted or dropped, so tracks at the same rate join seamlessly. If resampling, the same resampler carries on across the join.
- **Rate changes.** Before a track starts, `convert::choose_rate` picks the output rate: the track's own if the device offers it, otherwise the nearest rate in the same family (44.1k or 48k multiples). On Windows only the shared-mode mixer's rate is offered, so other rates are resampled here, visibly, rather than quietly by Windows. If the next track wants a different rate from the open output, the ring is played out, the output is reopened at the new rate, and the track starts (a short pause between albums at different rates, never a resample that isn't needed).
- **Adding to the list** (`Cmd::Append`) lengthens the queue. While the last track is still being decoded, the decoder reaches the new tracks by itself. If it had already run out of list but the last track is still playing (the end-of-list mark is written but not reached), it takes away the end mark and opens the next track straight on from the last, as if it had been there all along. Once the list has played out, adding starts nothing.
- **Jumps** (play a track, seek, stop, change device) flush the ring: the player thread sets `Shared::flush`, the callback empties the ring and counts what it dropped as played, and the player thread starts writing the new position.
- **Marks.** For every track it starts, the player thread records a `Mark`: where in the output stream (in frames) the track begins, its offset (after a seek), and plain facts about it (codec, rate, bits, the output rate, whether it's resampled). The panel compares these with `Shared::played`, the callback's running count of frames played, to know which track is audible now and how far in; so the display changes track exactly when the sound does, not when the decoder moved on. A last mark with no track marks the end of the list.

## The output callback

It runs on the audio thread and must never lock, allocate, log or touch files. Each time it:

1. empties the ring if a flush was asked for;
2. outputs silence if paused (pausing is instant: `Engine::send(Cmd::Pause)` sets the atomic directly);
3. copies what the ring holds, applying volume only if it isn't 0 dB, converts to the device's sample format, notes each side's peak, and feeds the spectrum tap;
4. pads with silence if the ring ran short, and counts a dropout only if the player thread said more was coming (`feeding`), so the end of a list and the pause before a rate change don't count.

## Decoding and conversion

- `decode::Source` wraps symphonia: probe, the first audio track, decode packet by packet to interleaved f32. Gapless trimming is on (`enable_gapless`), which trims LAME/Xing-tagged MP3s. M4A files that don't state their rate or channels have their first packet decoded while opening, and that block is handed out first.
- `convert::Converter` maps channels to the device's (mono to both sides, extra device channels silent) and, only if the rates differ, resamples with rubato's FFT resampler. With matching rates it copies the samples untouched (a unit test checks bit-for-bit).

## The panel (`app.rs`)

Laid out for 1180 × 720 and up, scaled down to fit smaller windows (never up: a bigger window gives the artwork, up to 1000 × 1000, and the list more room; the artwork's column also stops short of pushing the list's keys into the family switcher on the nameplate's row), drawn with TAW-Panel's `hifi` look. The Player draws its own title strip, window buttons and resize edges unless Settings turns the system title bar on. It polls the engine's `Status` (marks, output, messages, a new spectrum feed after the output reopens) each frame, the tag reader's results, and the inbox. While playing it repaints about 30 times a second; otherwise twice a second, to notice hand-overs from the Library.

## Media keys (`media.rs`)

souvlaki connects the Player to the desktop: MPRIS on Linux (over zbus, so there's no libdbus to build against), the System Media Transport Controls on Windows (which need the window's handle, taken when the panel starts). Keys arrive on souvlaki's thread, go down a channel and wake the panel, which acts on them as on its own keys. Each frame the panel tells it the playing track and the state; it sends only what changed, and the position about twice a second (MPRIS doesn't count the time itself). The artwork is found on a worker thread: the folder's cover as a `file://` URL, or else the file's picture written to `now-playing/` in the data folder.

## One Player at a time (`instance.rs`)

The running Player writes `running` (its process id and the time) to its data folder every second. A later launch with files checks it: if a Player checked in within three seconds, it writes the list (and whether `--play` was given) to `inbox/<time>-<pid>.json` and waits up to three seconds for the running Player to take (delete) it, then exits. If nobody takes it, it opens the files itself. No sockets, so nothing listens on the network.

## Family (`family.rs`)

`find` looks for `taw-library`, `taw-equaliser` and `taw-analyser` (with `.exe` on Windows): a location the user chose, next to the Player, in sibling folders named after the program (unzipped downloads side by side), the family's folders under Program Files and `%LOCALAPPDATA%\Programs` on Windows, then the PATH. `launch` starts it detached.

## Testing without a sound card

Unit tests cover the media keys and artwork URLs, the playlist, M3U parsing, the rate decision, the converter (untouched pass-through, resampled length and level), track location from marks, decoding, artwork search, family search and the hand-off.

To hear (or measure) real playback in a cloud session, run PipeWire with a null sink and record its monitor:

```sh
export XDG_RUNTIME_DIR=/tmp/xdg; mkdir -p -m 700 $XDG_RUNTIME_DIR
dbus-run-session -- sh -c 'pipewire & sleep 1; wireplumber & sleep 2;
  pw-cli create-node adapter "{ factory.name=support.null-audio-sink node.name=testsink media.class=Audio/Sink object.linger=true audio.position=[FL FR] }";
  sleep infinity' &
pw-record --target testsink -P '{ stream.capture.sink=true }' --format f32 out.wav &
DISPLAY=:99 cargo run --release -- tools/fixtures/gapless-flac
```

`cargo test --release -- --ignored follow_on` checks that tracks added near the end of the list follow on frame for frame (it needs the output at 44.1 kHz; see the next line). `playerctl -p taw_player status` (and `play`, `next`, `metadata`…) tries the media controls, in the same D-Bus session as the Player.

`pw-metadata -n settings 0 clock.allowed-rates "[ 44100 48000 96000 ]"` lets PipeWire follow the track's rate, to try the rate change. The test tracks are one continuous 440 Hz sine, so a gap shows up as a run of zeros and a click as a spike in the second difference of the recording.
