#!/usr/bin/env python3
"""Offline oracle for window statistics on the real speech clips in
crates/voice-core/tests/data/speech/.

Development tool only (see tools/oracle.py). Each clip is cut into consecutive 0.4 s
windows (a partial last window is dropped). References from Praat for every window:
  - level_dbfs: "Get root-mean-square" over the window, as 10 log10 of the mean square;
  - the voiced frames of Praat's pitch track ("To Pitch (cc)", 75 to 600 Hz, 0.01 s
    step) whose time lies in the window, from which the mean f0 and the 10th and 90th
    percentile (linear interpolation) are taken when there are at least 3 voiced frames.
Per clip: speech_level_dbfs (energy mean of the 'sounding' intervals of "To TextGrid
(silences)", 100 Hz, -25 dB, 0.1 s) and median_f0_hz of all voiced frames, the baselines
for the relative numbers.

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_window_stats.py [out.json]
"""
import json
import math
import sys
from pathlib import Path

import numpy as np

DATA = Path("crates/voice-core/tests/data")
WINDOW_S = 0.4
MIN_VOICED = 3


def main():
    import parselmouth
    from parselmouth.praat import call

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DATA / "speech_window_stats_oracle.json"
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "window_s": WINDOW_S,
            "pitch_floor_hz": 75,
            "pitch_ceiling_hz": 600,
            "time_step_s": 0.01,
            "min_voiced_frames": MIN_VOICED,
            "note": "per window: level_dbfs (RMS), mean_f0_hz, p10_hz, p90_hz (null with under 3 voiced frames)",
        },
        "clips": {},
    }
    for wav in sorted((DATA / "speech").glob("*.wav")):
        snd = parselmouth.Sound(str(wav))
        pitch = snd.to_pitch(time_step=0.01, pitch_floor=75, pitch_ceiling=600)
        f0 = pitch.selected_array["frequency"]
        xs = pitch.xs()
        grid = call(snd, "To TextGrid (silences)", 100.0, 0.0, -25.0, 0.1, 0.1, "silent", "sounding")
        energy, dur = 0.0, 0.0
        for i in range(1, call(grid, "Get number of intervals", 1) + 1):
            if call(grid, "Get label of interval", 1, i) != "sounding":
                continue
            t1 = call(grid, "Get start time of interval", 1, i)
            t2 = call(grid, "Get end time of interval", 1, i)
            rms = call(snd, "Get root-mean-square", t1, t2)
            energy += rms * rms * (t2 - t1)
            dur += t2 - t1
        windows = []
        for k in range(int(snd.duration // WINDOW_S)):
            t1, t2 = k * WINDOW_S, (k + 1) * WINDOW_S
            rms = call(snd, "Get root-mean-square", t1, t2)
            level = 10 * math.log10(max(rms * rms, 1e-12))
            hz = f0[(xs >= t1) & (xs < t2) & (f0 > 0)]
            w = {"level_dbfs": round(level, 3), "voiced_frames": int(len(hz))}
            if len(hz) >= MIN_VOICED:
                w.update(
                    mean_f0_hz=round(float(hz.mean()), 3),
                    p10_hz=round(float(np.percentile(hz, 10)), 3),
                    p90_hz=round(float(np.percentile(hz, 90)), 3),
                )
            windows.append(w)
        ref["clips"][wav.stem] = {
            "speech_level_dbfs": round(10 * math.log10(energy / dur), 3),
            "median_f0_hz": round(float(np.median(f0[f0 > 0])), 3),
            "windows": windows,
        }
    out.write_text(json.dumps(ref, indent=1) + "\n")
    print(f"wrote {out} ({len(ref['clips'])} clips)")


if __name__ == "__main__":
    main()
