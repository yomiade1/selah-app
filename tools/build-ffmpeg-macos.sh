#!/usr/bin/env bash
# Build the decode-only, LGPL, shared FFmpeg for macOS from the pinned source (M1.7).
#
# No LGPL macOS binary is published, so tools/third-party.py --extract unpacks the pinned
# FFmpeg 7.1.2 source (SHA-256 checked) and this script builds it offline into
#   third_party/ffmpeg/macos-arm64/shared/ffmpeg-n7.1.2-macos-arm64-decode-shared/
# with only the demuxers, decoders and protocols Selah allows (SEC-04) and VideoToolbox.
# Install names are absolute paths inside that folder, so tests need no DYLD_LIBRARY_PATH.
#
#   tools/build-ffmpeg-macos.sh [--if-missing]
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
BASE="$ROOT/third_party/ffmpeg/macos-arm64"
PREFIX="$BASE/shared/ffmpeg-n7.1.2-macos-arm64-decode-shared"
if [ "${1:-}" = "--if-missing" ] && [ -f "$PREFIX/lib/libavcodec.dylib" ]; then
  exit 0
fi
[ "$(uname -s)" = "Darwin" ] || { echo "build-ffmpeg-macos.sh runs on macOS only" >&2; exit 2; }
SOURCE="$(find "$BASE/source" -maxdepth 2 -name configure -path '*FFmpeg-n7.1.2*' -print -quit 2>/dev/null || true)"
if [ -z "$SOURCE" ]; then
  python3 tools/third-party.py --extract >/dev/null
  SOURCE="$(find "$BASE/source" -maxdepth 2 -name configure -path '*FFmpeg-n7.1.2*' -print -quit)"
fi
SOURCE="$(dirname "$SOURCE")"
BUILD="$ROOT/target/ffmpeg-macos-build"
rm -rf "$BUILD" "$PREFIX"
mkdir -p "$BUILD"
cd "$BUILD"
# Keep these lists in step with crates/selah-media/src/lib.rs (DEMUXERS, VIDEO_DECODERS,
# AUDIO_DECODERS, FILE_PROTOCOLS).
"$SOURCE/configure" \
  --prefix="$PREFIX" \
  --enable-shared --disable-static \
  --disable-programs --disable-doc --disable-debug \
  --disable-autodetect --disable-network \
  --disable-everything \
  --disable-avdevice --disable-avfilter --disable-swscale --disable-swresample \
  --enable-pthreads --enable-videotoolbox \
  --enable-protocol=file,pipe \
  --enable-demuxer=mov,matroska \
  --enable-decoder=h264,hevc,vp9,prores,aac,mp3float,opus,flac,pcm_s16le,pcm_s24le,pcm_f32le \
  --enable-parser=h264,hevc,vp9,aac,opus,flac,mpegaudio \
  --enable-hwaccel=h264_videotoolbox,hevc_videotoolbox,vp9_videotoolbox,prores_videotoolbox \
  --enable-bsf=h264_mp4toannexb,hevc_mp4toannexb,vp9_superframe_split
make -j"$(sysctl -n hw.ncpu)"
make install
cp "$SOURCE/COPYING.LGPLv2.1" "$PREFIX/LICENSE.txt"
echo "FFmpeg (decode-only, LGPL) installed in ${PREFIX#"$ROOT"/}"
