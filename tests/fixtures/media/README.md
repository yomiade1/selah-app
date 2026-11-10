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

## Committed clips

`committed/h264-1080p60.mp4` (about 350 KB) is checked in for the video correctness test
(`crates/selah-testkit/tests/video.rs`, M1-A7), because the macOS FFmpeg build is decode-only
and can't generate clips. It's 60 frames of FFmpeg's synthetic `testsrc2` pattern at 1080p60,
H.264 (OpenH264), no audio, with a white frame at frame 30.

`committed/flash-beep-360p60.mp4` (~80 KB) is checked in for cross-platform A/V sync verification
(M3-A4): 640×360 H.264 (OpenH264) at 60 fps + 48 kHz AAC audio with the white flash at frame 30
and a 1 kHz beep from 0.50 s to 0.52 s.

`committed/wcag-flash-360p60.mp4` is checked in for WCAG 2.3.1 strobe/flash detection tests
(M3-A8, SAFE-04): 640×360 H.264 (OpenH264) at 60 fps alternating full-frame black/white at 6 Hz
(above the 3 Hz threshold). Regenerate committed fixtures with
`python tools/generate-media.py --committed`.
