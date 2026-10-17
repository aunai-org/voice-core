#!/usr/bin/env python3
"""Offline oracle for the emphasis measure on the real speech clips in
crates/voice-core/tests/data/speech/.

Development tool only (see tools/oracle.py). Everything is taken from Praat, with none
of voice-core's code:
  - speech segments: the 'sounding' intervals of "To TextGrid (silences)" (100 Hz, -25 dB,
    0.1 s, 0.1 s); the gaps between consecutive sounding intervals are the pauses;
  - speech level: energy mean of the sounding intervals ("Get root-mean-square");
  - pitch: "To Pitch (cc)", 75 to 600 Hz, 0.01 s step; median f0 of all voiced frames;
  - a window's relative level is 10 log10 of its mean square minus the speech level, its
    relative pitch is the mean f0 of its voiced frames (3 or more) against the median f0
    in semitones;
  - typical range: 25th, 50th and 75th percentile (linear interpolation) of the two
    relative numbers over consecutive 0.4 s windows inside the sounding intervals (each
    interval from its start), when at least 5 windows have the value;
  - test words: 0.4 s windows from 0.2 s in steps of 0.8 s that fit in the clip; per
    word the two relative numbers and the pause before and after (the gap whose end /
    start is nearest the word's start / end and within 0.2 s, else 0).

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_emphasis.py [out.json]
"""
import json
import math
import sys
from pathlib import Path

import numpy as np

DATA = Path("crates/voice-core/tests/data")
WINDOW_S = 0.4
MIN_VOICED = 3
MIN_TYPICAL = 5
REACH_S = 0.2


def band(values):
    if len(values) < MIN_TYPICAL:
        return None
    return [round(float(np.percentile(values, q)), 4) for q in (25, 50, 75)]


def main():
    import parselmouth
    from parselmouth.praat import call

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DATA / "speech_emphasis_oracle.json"
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "window_s": WINDOW_S,
            "reach_s": REACH_S,
            "note": "per clip: typical_level_db / typical_pitch_st = [p25, p50, p75] or null; "
            "words: start_s, end_s, relative_level_db, relative_pitch_st (null if no pitch), pause_before_s, pause_after_s",
        },
        "clips": {},
    }
    for wav in sorted((DATA / "speech").glob("*.wav")):
        snd = parselmouth.Sound(str(wav))
        pitch = snd.to_pitch(time_step=0.01, pitch_floor=75, pitch_ceiling=600)
        f0 = pitch.selected_array["frequency"]
        xs = pitch.xs()
        median_f0 = float(np.median(f0[f0 > 0]))
        grid = call(snd, "To TextGrid (silences)", 100.0, 0.0, -25.0, 0.1, 0.1, "silent", "sounding")
        sounding = []
        for i in range(1, call(grid, "Get number of intervals", 1) + 1):
            if call(grid, "Get label of interval", 1, i) == "sounding":
                sounding.append((
                    call(grid, "Get start time of interval", 1, i),
                    call(grid, "Get end time of interval", 1, i),
                ))
        energy = sum(call(snd, "Get root-mean-square", a, b) ** 2 * (b - a) for a, b in sounding)
        speech_level = 10 * math.log10(energy / sum(b - a for a, b in sounding))
        gaps = [(sounding[i][1], sounding[i + 1][0]) for i in range(len(sounding) - 1)]

        def measure(t1, t2):
            rms = call(snd, "Get root-mean-square", t1, t2)
            level = 10 * math.log10(max(rms * rms, 1e-12)) - speech_level
            hz = f0[(xs >= t1) & (xs < t2) & (f0 > 0)]
            pitch_st = 12 * math.log2(hz.mean() / median_f0) if len(hz) >= MIN_VOICED else None
            return level, pitch_st

        typical = []
        for a, b in sounding:
            t = a
            while t + WINDOW_S <= b + 1e-9:
                typical.append(measure(t, t + WINDOW_S))
                t += WINDOW_S

        def near(at, edge):
            c = [(abs(edge(g) - at), g) for g in gaps if abs(edge(g) - at) <= REACH_S]
            return round(min(c, key=lambda p: p[0])[1][1] - min(c, key=lambda p: p[0])[1][0], 4) if c else 0.0

        words = []
        k = 0
        while 0.2 + 0.8 * k + WINDOW_S <= snd.duration:
            t1 = 0.2 + 0.8 * k
            t2 = t1 + WINDOW_S
            level, pitch_st = measure(t1, t2)
            words.append({
                "start_s": round(t1, 4),
                "end_s": round(t2, 4),
                "relative_level_db": round(level, 4),
                "relative_pitch_st": None if pitch_st is None else round(pitch_st, 4),
                "pause_before_s": near(t1, lambda g: g[1]),
                "pause_after_s": near(t2, lambda g: g[0]),
            })
            k += 1
        ref["clips"][wav.stem] = {
            "typical_windows": len(typical),
            "typical_level_db": band([l for l, _ in typical]),
            "typical_pitch_st": band([p for _, p in typical if p is not None]),
            "words": words,
        }
    out.write_text(json.dumps(ref, indent=1) + "\n")
    print(f"wrote {out} ({len(ref['clips'])} clips)")


if __name__ == "__main__":
    main()
