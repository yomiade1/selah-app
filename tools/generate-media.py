#!/usr/bin/env python3
"""Generate synthetic, licence-clean H.264/HEVC fixtures without network access."""
import argparse
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def run(arguments):
    return subprocess.run(arguments, check=True, capture_output=True, timeout=300)


def generate(ffmpeg, destination):
    version = run([str(ffmpeg), "-version"]).stdout.decode("utf-8")
    if "--enable-gpl" in version or "--enable-shared" not in version or "version n7." not in version:
        raise ValueError("fixtures require the pinned LGPL shared FFmpeg 7 build")
    encoders = run([str(ffmpeg), "-hide_banner", "-encoders"]).stdout.decode("utf-8")
    if "libopenh264" not in encoders or "libkvazaar" not in encoders:
        raise ValueError("the pinned build must have libopenh264 and libkvazaar encoders")
    ffprobe = ffmpeg.with_name("ffprobe.exe" if os.name == "nt" else "ffprobe")
    destination.mkdir(parents=True, exist_ok=True)
    clips = []
    for width, height in [(1920, 1080), (3840, 2160)]:
        for codec, encoder in [("h264", "libopenh264"), ("hevc", "libkvazaar")]:
            name = f"{codec}-{height}p60.mp4"
            target = destination / name
            temporary = destination / f"{codec}-{height}p60.partial.mp4"
            video = (f"testsrc2=size={width}x{height}:rate=60:duration=1,"
                     "drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable='eq(n,30)'")
            audio = "aevalsrc=if(between(t\\,0.5\\,0.52)\\,0.7*sin(2*PI*1000*t)\\,0):s=48000:d=1"
            arguments = [str(ffmpeg), "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
                         "-f", "lavfi", "-i", video, "-f", "lavfi", "-i", audio,
                         "-map", "0:v", "-map", "1:a", "-c:v", encoder,
                         "-b:v", "8000k", "-g", "60", "-threads", "1", "-pix_fmt", "yuv420p",
                         "-c:a", "aac", "-b:a", "192k", "-frames:v", "60", "-t", "1",
                         "-map_metadata", "-1"]
            if codec == "hevc":
                arguments += ["-preset", "ultrafast", "-tag:v", "hvc1"]
            arguments += [str(temporary)]
            try:
                run(arguments)
                probe = json.loads(run([str(ffprobe), "-v", "error", "-count_frames",
                                        "-show_streams", "-of", "json", str(temporary)]).stdout)
                stream = next(s for s in probe["streams"] if s["codec_type"] == "video")
                if (stream["width"], stream["height"], stream["codec_name"],
                        stream["r_frame_rate"], int(stream["nb_read_frames"])) != \
                        (width, height, codec, "60/1", 60):
                    raise ValueError(f"unexpected generated stream: {name}")
                temporary.replace(target)
            finally:
                temporary.unlink(missing_ok=True)
            clips.append({"file": name, "codec": codec, "width": width, "height": height,
                          "frames": 60, "fps": 60, "flash_frame": 30, "beep_start_sample": 24000,
                          "sample_rate": 48000})
            print(f"generated and decoded {name}: 60 frames", flush=True)
    manifest = {"generator": 1, "ffmpeg": version.splitlines()[0], "clips": clips}
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def committed(ffmpeg, destination):
    """Small clips committed to the repository for deterministic CI tests across all OSes.

    macOS has no encoder in its decode-only FFmpeg, so it can't generate clips; these are
    committed instead:
    - h264-1080p60.mp4: 1080p60 H.264, 60 frames, white flash at frame 30, no audio (M1-A7).
    - flash-beep-360p60.mp4: 640x360 H.264 + AAC 48 kHz, white flash at frame 30 + 1 kHz beep at
      0.50-0.52 s (M3-A4 cross-platform A/V sync).
    """
    version = run([str(ffmpeg), "-version"]).stdout.decode("utf-8")
    if "--enable-gpl" in version:
        raise ValueError("fixtures require the pinned LGPL FFmpeg build")
    destination.mkdir(parents=True, exist_ok=True)
    target = destination / "h264-1080p60.mp4"
    if not target.exists():
        video = ("testsrc2=size=1920x1080:rate=60:duration=1,"
                 "drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable='eq(n,30)'")
        run([str(ffmpeg), "-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-f", "lavfi",
             "-i", video, "-c:v", "libopenh264", "-b:v", "1500k", "-g", "60", "-threads", "1",
             "-pix_fmt", "yuv420p", "-frames:v", "60", "-an", "-map_metadata", "-1",
             "-fflags", "+bitexact", "-flags:v", "+bitexact", str(target)])
        print(f"generated {target.relative_to(ROOT)} ({target.stat().st_size} bytes)")

    av_target = destination / "flash-beep-360p60.mp4"
    if not av_target.exists():
        av_video = ("testsrc2=size=640x360:rate=60:duration=1,"
                    "drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable='eq(n,30)'")
        av_audio = "aevalsrc=if(between(t\\,0.5\\,0.52)\\,0.7*sin(2*PI*1000*t)\\,0):s=48000:d=1"
        run([str(ffmpeg), "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
             "-f", "lavfi", "-i", av_video, "-f", "lavfi", "-i", av_audio,
             "-map", "0:v", "-map", "1:a", "-c:v", "libopenh264", "-b:v", "400k",
             "-g", "60", "-threads", "1", "-pix_fmt", "yuv420p",
             "-c:a", "aac", "-b:a", "96k", "-frames:v", "60", "-t", "1",
             "-map_metadata", "-1", "-fflags", "+bitexact", "-flags:v", "+bitexact",
             str(av_target)])
        print(f"generated {av_target.relative_to(ROOT)} ({av_target.stat().st_size} bytes)")

    strobe_target = destination / "wcag-flash-360p60.mp4"
    if not strobe_target.exists():
        strobe_video = ("color=c=black:s=640x360:rate=60:d=1,"
                        "drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable='lt(mod(n\\,10)\\,5)'")
        run([str(ffmpeg), "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
             "-f", "lavfi", "-i", strobe_video,
             "-c:v", "libopenh264", "-b:v", "400k", "-g", "60", "-threads", "1",
             "-pix_fmt", "yuv420p", "-frames:v", "60", "-an",
             "-map_metadata", "-1", "-fflags", "+bitexact", "-flags:v", "+bitexact",
             str(strobe_target)])
        print(f"generated {strobe_target.relative_to(ROOT)} ({strobe_target.stat().st_size} bytes)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ffmpeg", type=Path)
    parser.add_argument("--committed", action="store_true",
                        help="regenerate the small committed clip in tests/fixtures/media/committed/")
    args = parser.parse_args()
    ffmpeg = args.ffmpeg
    if ffmpeg is None:
        name = "ffmpeg.exe" if os.name == "nt" else "ffmpeg"
        candidates = list((ROOT / "third_party/ffmpeg").rglob(f"bin/{name}"))
        if len(candidates) != 1:
            parser.error("use --ffmpeg to select one extracted pinned FFmpeg binary")
        ffmpeg = candidates[0]
    if args.committed:
        committed(ffmpeg.resolve(), ROOT / "tests/fixtures/media/committed")
        return
    generate(ffmpeg.resolve(), ROOT / "tests/fixtures/media/generated")


if __name__ == "__main__":
    main()
