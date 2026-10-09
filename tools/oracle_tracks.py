#!/usr/bin/env python3
"""Offline oracle for moving-pitch signals: frame-by-frame Praat f0 tracks.

Same GPL rules as tools/oracle.py: development tool only, never built into or
published with voice-core, no Praat code copied. The signals are defined by
closed-form phase formulas that the Rust test (tests/pitch_tracks.rs)
re-implements, so no audio files are needed.

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_tracks.py [out.json]
"""
import json
import math
import sys
from pathlib import Path

import numpy as np

SR = 16_000
SECS = 2.0
T = np.arange(int(SR * SECS)) / SR
TAU = 2 * math.pi


def glide():
    # f(t) = 100 + 50 t  -> 100..200 Hz
    return 0.5 * np.sin(TAU * (100 * T + 25 * T**2)), 100 + 50 * T


def vibrato():
    # f(t) = 150 (1 + 0.05 sin(2 pi 5 t)); phase integral done in closed form
    ph = TAU * 150 * T - 1.5 * np.cos(TAU * 5 * T)
    return 0.5 * np.sin(ph), 150 * (1 + 0.05 * np.sin(TAU * 5 * T))


def harmonic_glide():
    # five harmonics, 1/k roll-off, f0 120..240 Hz
    ph = TAU * (120 * T + 30 * T**2)
    x = sum(np.sin(k * ph) / k for k in range(1, 6))
    return 0.3 * x, 120 + 60 * T


SIGNALS = {"glide": glide, "vibrato": vibrato, "harmonic_glide": harmonic_glide}


def main():
    import parselmouth

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("crates/voice-core/tests/data/pitch_tracks_oracle.json")
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "pitch_floor_hz": 75,
            "pitch_ceiling_hz": 600,
            "time_step_s": 0.01,
            "note": "f0 0.0 = unvoiced in Praat; true_hz is the signal's instantaneous frequency at the frame time",
        },
        "clips": {},
    }
    for name, fn in SIGNALS.items():
        x, f_true = fn()
        snd = parselmouth.Sound(x, sampling_frequency=SR)
        p = snd.to_pitch(time_step=0.01, pitch_floor=75, pitch_ceiling=600)
        times = p.xs()
        f0 = p.selected_array["frequency"]
        true_at = np.interp(times, T, f_true)
        ref["clips"][name] = {
            "times_s": [round(float(t), 6) for t in times],
            "praat_hz": [round(float(v), 4) for v in f0],
            "true_hz": [round(float(v), 4) for v in true_at],
        }
    out.write_text(json.dumps(ref) + "\n")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
