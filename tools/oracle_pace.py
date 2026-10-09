#!/usr/bin/env python3
"""Offline oracle for syllable-nucleus counts: Praat intensity and pitch plus the
de Jong & Wempe (2009) peak rules, written independently of the Rust code.

Same GPL rules as tools/oracle.py: development tool only, never built into or
published with voice-core, no Praat code or script copied. Praat supplies the
intensity contour (Gaussian window, 50 Hz minimum pitch) and the voicing from its
own pitch tracker; the peak rules below are re-implemented from the paper. This is
therefore a check against Praat's measurements plus an independent reading of the
rules, not a Praat-script oracle.

The clips are closed-form (raised-cosine tone bursts) and are rebuilt in
tests/pace_reference.rs from the same formulas.

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_pace.py [out.json]
"""
import json
import math
import sys
from pathlib import Path

import numpy as np

SR = 16_000
CLIPS = {
    "bursts_3hz": {"rate": 3.0, "segments": [[0.0, 3.0]], "secs": 3.0},
    "bursts_4hz": {"rate": 4.0, "segments": [[0.0, 3.0]], "secs": 3.0},
    "bursts_5hz": {"rate": 5.0, "segments": [[0.0, 3.0]], "secs": 3.0},
    "bursts_6hz": {"rate": 6.0, "segments": [[0.0, 3.0]], "secs": 3.0},
    "bursts_pause": {"rate": 4.0, "segments": [[0.0, 1.0], [2.5, 3.5]], "secs": 3.5},
}


def make(spec):
    t = np.arange(int(SR * spec["secs"])) / SR
    env = np.zeros_like(t)
    for a, b in spec["segments"]:
        m = (t >= a) & (t < b)
        env[m] = 0.5 * (1 - np.cos(2 * math.pi * spec["rate"] * (t[m] - a)))
    return 0.5 * env * np.sin(2 * math.pi * 150 * t)


def nuclei(x):
    import parselmouth

    snd = parselmouth.Sound(x, sampling_frequency=SR)
    inten = snd.to_intensity(minimum_pitch=50, time_step=0.01)
    levels = inten.values[0]
    times = inten.xs()
    pitch = snd.to_pitch(time_step=0.01, pitch_floor=75, pitch_ceiling=600)
    f0 = pitch.selected_array["frequency"]
    ptimes = pitch.xs()

    def voiced(t):
        return f0[int(np.argmin(np.abs(ptimes - t)))] > 0

    threshold = np.quantile(levels, 0.99) - 25.0
    kept = []
    for i in range(1, len(levels) - 1):
        if not (levels[i] > levels[i - 1] and levels[i] >= levels[i + 1]):
            continue
        if levels[i] < threshold or not voiced(times[i]):
            continue
        if not kept:
            kept.append(i)
            continue
        p = kept[-1]
        dip = levels[p : i + 1].min()
        if levels[p] - dip >= 2.0 and levels[i] - dip >= 2.0:
            kept.append(i)
        elif levels[i] > levels[p]:
            kept[-1] = i
    return [round(float(times[i]), 4) for i in kept]


def main():
    import parselmouth

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("crates/voice-core/tests/data/pace_oracle.json")
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "intensity": "to_intensity(minimum_pitch=50, time_step=0.01)",
            "pitch": "to_pitch(time_step=0.01, floor 75, ceiling 600)",
            "rules": "99th percentile - 25 dB, dip >= 2 dB, voiced",
        },
        "clips": {},
    }
    for name, spec in CLIPS.items():
        n = nuclei(make(spec))
        truth = int(round(sum((b - a) * spec["rate"] for a, b in spec["segments"])))
        ref["clips"][name] = {"true_count": truth, "praat_count": len(n), "praat_times_s": n}
    out.write_text(json.dumps(ref, indent=1) + "\n")
    print(f"wrote {out}")
    for k, v in ref["clips"].items():
        print(k, "true", v["true_count"], "praat-based", v["praat_count"])


if __name__ == "__main__":
    main()
