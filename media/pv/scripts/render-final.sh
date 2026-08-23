#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SILENT="$ROOT_DIR/dist/openbaud-pv-silent.mp4"
AUDIO="$ROOT_DIR/dist/openbaud-pv-bed.wav"
FINAL="$ROOT_DIR/docs/assets/openbaud-pv.mp4"
GIF="$ROOT_DIR/docs/assets/openbaud-pv-demo.gif"

mkdir -p "$ROOT_DIR/dist" "$ROOT_DIR/docs/assets"

ffmpeg -y -loglevel error \
  -f lavfi -t 46 -i "sine=frequency=55:sample_rate=48000" \
  -f lavfi -t 46 -i "sine=frequency=82.5:sample_rate=48000" \
  -f lavfi -t 46 -i "sine=frequency=110:sample_rate=48000" \
  -f lavfi -t 46 -i "anoisesrc=color=pink:amplitude=0.025:sample_rate=48000" \
  -f lavfi -t 0.5 -i "sine=frequency=660:sample_rate=48000" \
  -f lavfi -t 0.5 -i "sine=frequency=880:sample_rate=48000" \
  -filter_complex \
  "[0:a]volume=0.10,lowpass=f=180[a0];[1:a]volume=0.055,lowpass=f=260[a1];[2:a]volume=0.025,lowpass=f=360[a2];[3:a]lowpass=f=900,highpass=f=90,volume=0.18[a3];[4:a]volume=0.10,afade=t=out:st=0.06:d=0.42,adelay=4000|4000[p1];[5:a]volume=0.09,afade=t=out:st=0.06:d=0.42,adelay=11000|11000[p2];[4:a]volume=0.10,afade=t=out:st=0.06:d=0.42,adelay=22000|22000[p3];[5:a]volume=0.09,afade=t=out:st=0.06:d=0.42,adelay=31000|31000[p4];[4:a]volume=0.12,afade=t=out:st=0.06:d=0.42,adelay=39000|39000[p5];[5:a]volume=0.11,afade=t=out:st=0.06:d=0.42,adelay=43000|43000[p6];[a0][a1][a2][a3][p1][p2][p3][p4][p5][p6]amix=inputs=10:normalize=0,afade=t=in:st=0:d=1.5,afade=t=out:st=43:d=3,loudnorm=I=-20:LRA=7:TP=-2[a]" \
  -map "[a]" -c:a pcm_s16le "$AUDIO"

ffmpeg -y -loglevel error -i "$SILENT" -i "$AUDIO" \
  -map 0:v:0 -map 1:a:0 -c:v copy -c:a aac -b:a 192k -shortest \
  -movflags +faststart "$FINAL"

ffmpeg -y -loglevel error -ss 11 -t 10 -i "$FINAL" \
  -filter_complex \
  "fps=10,scale=960:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=128:stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=4" \
  -loop 0 "$GIF"

echo "Wrote $FINAL and $GIF"
