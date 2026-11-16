# Video Encoding

The video test measures how fast the machine can convert video, which decides whether it is suitable as a media server (transcoding for phones, TVs and remote viewers).

It needs [`ffmpeg`](https://ffmpeg.org) in the `PATH`. The program starts `ffmpeg` as a separate program; nothing of it is built into `syscheck`. Without it the test is skipped and the report tells you so.

```
syscheck --only video
```

## The workload

`ffmpeg` generates a synthetic test picture (`testsrc2`) in 1920x1080 at 30 frames per second and encodes it. The input is the same on every machine, so no video file is needed and the result does not depend on a file.

| Encoder | Examples | Tested |
| --- | --- | --- |
| CPU | `libx264` (H.264), `libx265` (HEVC, full run only) | when `ffmpeg` has it |
| Hardware | NVENC (NVIDIA), Quick Sync (Intel), VAAPI (Linux), AMF (AMD), VideoToolbox (Apple), MediaCodec (Android) | only when it **actually works** on this machine: the program tries each one and reports "not working" when the hardware, driver or `ffmpeg` support is missing |

## Results

For every working encoder the report shows:

* **fps:** frames per second for one encode. 30 fps equals one real-time 1080p30 stream.
* **4x fps** (full run only): the combined speed of four encodes running at the same time.
* **Streams:** how many real-time (30 fps) 1080p30 encodes fit, which is the number that matters for a media server.

### How the speed is measured

The speed is the difference between a short and a longer run, so the start-up time of the encoder cancels out. The generator of the test picture also uses some CPU time. The program measures its speed too (`source fps`) and marks an encoder with `*` when it is close to that speed: the real limit of that encoder may then be higher than shown.

## Why hardware encoders matter

A CPU encoder is flexible and gives good quality but needs many cores. A hardware encoder does the same job with little CPU load and power. If no hardware encoder works, the report says that transcoding will run on the CPU. If `ffmpeg` shows an encoder but it fails, the usual causes are a missing driver, a container without access to the GPU, or an `ffmpeg` build without that encoder.

## Scores

The media server score uses the CPU encoder speed, the hardware encoder stream count, the multi-core CPU score, the disk stream capacity, and a few other factors. See [Scores and Verdicts](Scores-and-Verdicts.md).
