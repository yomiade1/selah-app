# Synthetic media

Run `python tools/generate-media.py` after extracting the pinned FFmpeg build.
It creates 1-second, 60 fps H.264 and HEVC clips at 1080p and 4K in `generated/`.
Each clip contains a white frame and a 1 kHz beep starting at 0.5 seconds.
They contain no church recordings, personal data or third-party creative work.

The generator requires the LGPL shared FFmpeg 7 build, OpenH264 (BSD) and
Kvazaar (BSD). It refuses GPL builds. Every generated clip is decoded with
ffprobe to verify its codec, resolution, frame rate and frame count.
The JSON manifest records the intended flash and beep positions; A/V sync
tests must measure the decoded signal, including codec delay.

Generated binaries are ignored by Git and can be rebuilt offline.

## Committed clip

`committed/h264-1080p60.mp4` (about 350 KB) is checked in for the video correctness test
(`crates/selah-testkit/tests/video.rs`, M1-A7), because the macOS FFmpeg build is decode-only
and can't generate clips. It's 60 frames of FFmpeg's synthetic `testsrc2` pattern at 1080p60,
H.264 (OpenH264), no audio, with a white frame at frame 30. Regenerate it with
`python tools/generate-media.py --committed`.
