#!/usr/bin/env python3
"""Generate synthetic known-answer WAV fixtures (stdlib only).

Writes 16 kHz mono 16-bit WAVs plus fixtures/manifest.json describing what the
true answer is for each clip. Real speech fixtures are a separate task.
Usage: python3 tools/gen_fixtures.py [out_dir]   (default: fixtures/synthetic)
"""
import json
import math
import random
import struct
import sys
import wave
from pathlib import Path

SR = 16_000


def write_wav(path, samples, sr=SR):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(b"".join(struct.pack("<h", max(-32768, min(32767, round(s * 32767)))) for s in samples))


def sine(freq, amp, secs, sr=SR):
    return [amp * math.sin(2 * math.pi * freq * i / sr) for i in range(int(sr * secs))]


def harmonic(f0, amp, secs, n=8):
    """Glottal-like harmonic stack with 1/k amplitude roll-off."""
    out = [0.0] * int(SR * secs)
    for k in range(1, n + 1):
        for i in range(len(out)):
            out[i] += (amp / k) * math.sin(2 * math.pi * f0 * k * i / SR)
    peak = max(abs(x) for x in out) or 1.0
    return [x * amp / peak for x in out]


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "fixtures/synthetic")
    out.mkdir(parents=True, exist_ok=True)
    rng = random.Random(1234)
    clips = {}

    def add(name, samples, truth, sr=SR):
        write_wav(out / f"{name}.wav", samples, sr)
        clips[name] = {"file": f"{name}.wav", "sample_rate": sr, "seconds": len(samples) / sr, **truth}

    for f in (100, 220, 440):
        add(f"sine_{f}hz", sine(f, 0.5, 2.0), {"f0_hz": f, "rms_dbfs": 20 * math.log10(0.5 / math.sqrt(2))})
    # 48 kHz clip for resampler checks (resampled to 16 kHz by the code under test)
    add("sine_440hz_48k", sine(440, 0.5, 2.0, 48_000), {"f0_hz": 440, "rms_dbfs": 20 * math.log10(0.5 / math.sqrt(2))}, 48_000)
    for f in (110, 200):
        add(f"harmonic_{f}hz", harmonic(f, 0.5, 2.0), {"f0_hz": f})
    add("silence", [0.0] * (SR * 2), {"f0_hz": None, "rms_dbfs": -120.0, "voiced": False})
    add("white_noise", [rng.uniform(-0.3, 0.3) for _ in range(SR * 2)], {"f0_hz": None, "voiced": False})
    # speech-like pattern: 0.5 s tone, 1.5 s pause, 0.5 s tone (pause count = 1, longest pause = 1.5 s)
    pat = sine(150, 0.4, 0.5) + [0.0] * int(SR * 1.5) + sine(150, 0.4, 0.5)
    add("tone_pause_tone", pat, {"f0_hz": 150, "pauses": 1, "longest_pause_s": 1.5})

    (out / "manifest.json").write_text(json.dumps({"sample_rate": SR, "clips": clips}, indent=2))
    print(f"wrote {len(clips)} clips to {out}")


if __name__ == "__main__":
    main()
