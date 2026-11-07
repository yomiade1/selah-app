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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ffmpeg", type=Path)
    args = parser.parse_args()
    ffmpeg = args.ffmpeg
    if ffmpeg is None:
        name = "ffmpeg.exe" if os.name == "nt" else "ffmpeg"
        candidates = list((ROOT / "third_party/ffmpeg").rglob(f"bin/{name}"))
        if len(candidates) != 1:
            parser.error("use --ffmpeg to select one extracted pinned FFmpeg binary")
        ffmpeg = candidates[0]
    generate(ffmpeg.resolve(), ROOT / "tests/fixtures/media/generated")


if __name__ == "__main__":
    main()
