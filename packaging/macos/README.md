# macOS test build

Not part of the releases yet: a test build for trying the Player on a Mac.

`.github/workflows/macos-test.yml` builds it on GitHub's Mac runners when
anything here (or that workflow) changes on a branch, or by hand from the
Actions tab. It makes one universal app (Apple silicon and Intel), `TAW
Player.app`, with this folder's `Info.plist` and an icon drawn by
`make-icon.py`, signs it ad hoc (no Apple Developer ID, so macOS asks once
before opening it), and zips it with `READ ME FIRST.txt` as the run's
`taw-player-<version>-macos` artifact (kept 7 days).

Known gaps on the Mac: the window has the Player's own title strip and
buttons (Settings can switch to the system's); "Open With" from Finder
isn't wired up (drop files on the window instead); the family switcher
won't find the other apps, which don't have Mac builds.
