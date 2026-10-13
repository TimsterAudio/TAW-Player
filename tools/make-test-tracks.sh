#!/usr/bin/env bash
# Builds test tracks in tools/fixtures (needs ffmpeg). Each folder is one
# continuous 440 Hz sine cut into tracks at exact sample positions, so any
# gap or click at a join shows up in the output.
#
#   gapless-flac   three FLAC tracks, 16-bit 44.1 kHz
#   gapless-wav    three WAV tracks, 24-bit 96 kHz
#   gapless-mp3    three MP3 tracks with LAME headers (encoder delay trimmed)
#   formats        one short track each: AIFF, ALAC and AAC M4A, OGG Vorbis,
#                  mono MP3, and 48 kHz FLAC (a rate change mid-list)
#   tagged         two FLACs with tags and a cover.jpg
set -euo pipefail
cd "$(dirname "$0")"
out=fixtures
rm -rf "$out"
mkdir -p "$out"/{gapless-flac,gapless-wav,gapless-mp3,formats,tagged}

# cut <rate> <first sample> <samples> <out file> [ffmpeg options...]
cut() {
    local rate=$1 start=$2 n=$3 file=$4
    shift 4
    ffmpeg -loglevel error -y -f lavfi -i "sine=frequency=440:sample_rate=$rate:duration=40" \
        -af "atrim=start_sample=$start:end_sample=$((start + n)),asetpts=N/SR/TB,pan=stereo|c0=c0|c1=c0" \
        "$@" "$file"
}

# Odd lengths on purpose, so joins don't fall on block boundaries.
cut 44100 0      220001 "$out/gapless-flac/01 Part one.flac"
cut 44100 220001 175333 "$out/gapless-flac/02 Part two.flac"
cut 44100 395334 233777 "$out/gapless-flac/03 Part three.flac"

cut 96000 0      300007 "$out/gapless-wav/01 Part one.wav" -c:a pcm_s24le
cut 96000 300007 288211 "$out/gapless-wav/02 Part two.wav" -c:a pcm_s24le

cut 44100 0      220001 "$out/gapless-mp3/01 Part one.mp3" -c:a libmp3lame -b:a 256k
cut 44100 220001 175333 "$out/gapless-mp3/02 Part two.mp3" -c:a libmp3lame -b:a 256k
cut 44100 395334 233777 "$out/gapless-mp3/03 Part three.mp3" -c:a libmp3lame -b:a 256k

cut 44100 0 132300 "$out/formats/01 AIFF.aiff"
cut 44100 0 132300 "$out/formats/02 ALAC.m4a" -c:a alac
cut 44100 0 132300 "$out/formats/03 AAC.m4a" -c:a aac -b:a 192k
cut 44100 0 132300 "$out/formats/04 Vorbis.ogg" -c:a libvorbis
ffmpeg -loglevel error -y -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=3" \
    -c:a libmp3lame -ac 1 "$out/formats/05 Mono.mp3"
cut 48000 0 144000 "$out/formats/06 Rate change 48k.flac"

ffmpeg -loglevel error -y -f lavfi -i "color=c=0xd8432a:s=600x600" -frames:v 1 "$out/tagged/cover.jpg"
for n in 1 2; do
    ffmpeg -loglevel error -y -f lavfi -i "sine=frequency=$((330 * n)):sample_rate=44100:duration=8" \
        -metadata title="Test Song $n" -metadata artist="The Test Tones" \
        -metadata album="Sine Waves Vol. 1" -metadata track=$n \
        "$out/tagged/0$n.flac"
done
echo "Test tracks are in tools/$out"
