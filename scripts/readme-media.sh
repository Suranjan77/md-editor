#!/usr/bin/env bash
# Regenerate the README screenshots and GIF in images/ from a headless run of the
# real app over a demo vault (native/src/e2e/showcase.rs). Needs ffmpeg.
set -euo pipefail
cd "$(dirname "$0")/.."

out=target/readme-media
cargo test -p md-editor-native readme_media -- --ignored --exact e2e::showcase::readme_media

# 12 fps, 960 px wide, one palette for the whole clip.
ffmpeg -v error -y -f concat -safe 0 -i "$out/gif-frames/frames.txt" \
  -vf "fps=12,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=200:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" \
  -loop 0 "$out/intro.gif"

mkdir -p images
cp "$out"/{editor,split-dark,split-light,completion,graph,palette,daily,tracker}.png "$out/intro.gif" images/
echo "Updated images/ from $out"
