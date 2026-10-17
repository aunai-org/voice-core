#!/usr/bin/env python3
"""Offline oracle for maximum phonation time on the real speech clips in
crates/voice-core/tests/data/speech/.

Development tool only (see tools/oracle.py). Praat has no maximum-phonation-time command,
so the reference applies the same stretch rule as src/phonation.rs (gaps of up to 2
unvoiced frames do not break a stretch, at least 10 voiced frames) to Praat's own pitch
track ("To Pitch (cc)", 75 to 600 Hz, 0.01 s step). It therefore checks the pitch
track and voicing decisions behind the measure, not the rule itself.

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_mpt.py [out.json]
"""
import json
import math
import sys
from pathlib import Path

DATA = Path("crates/voice-core/tests/data")
MAX_GAP = 2
MIN_FRAMES = 10


def longest_stretch(f0):
    """(first, last) frame indexes of the longest stretch, or None."""
    best = cur = None
    gap = 0
    for i, v in enumerate(f0):
        if v > 0:
            cur = (cur[0], i) if cur else (i, i)
            gap = 0
        elif cur:
            gap += 1
            if gap > MAX_GAP:
                if best is None or cur[1] - cur[0] > best[1] - best[0]:
                    best = cur
                cur = None
    if cur and (best is None or cur[1] - cur[0] > best[1] - best[0]):
        best = cur
    return best


def main():
    import parselmouth

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DATA / "speech_mpt_oracle.json"
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "pitch_floor_hz": 75,
            "pitch_ceiling_hz": 600,
            "time_step_s": 0.01,
            "max_gap_frames": MAX_GAP,
            "min_frames": MIN_FRAMES,
            "note": "longest voiced stretch of Praat's pitch track; null when none has 10 voiced frames",
        },
        "clips": {},
    }
    for wav in sorted((DATA / "speech").glob("*.wav")):
        snd = parselmouth.Sound(str(wav))
        p = snd.to_pitch(time_step=0.01, pitch_floor=75, pitch_ceiling=600)
        f0 = p.selected_array["frequency"]
        times = p.xs()
        s = longest_stretch(f0)
        entry = None
        if s:
            hz = [v for v in f0[s[0] : s[1] + 1] if v > 0]
            if len(hz) >= MIN_FRAMES:
                mean = sum(hz) / len(hz)
                st = [12 * math.log2(h / mean) for h in hz]
                m = sum(st) / len(st)
                sd = math.sqrt(sum((x - m) ** 2 for x in st) / len(st))
                entry = {
                    "duration_s": round((s[1] - s[0] + 1) * 0.01, 3),
                    "start_s": round(float(times[s[0]]) - 0.005, 3),
                    "mean_f0_hz": round(mean, 2),
                    "f0_sd_semitones": round(sd, 3),
                }
        ref["clips"][wav.stem] = entry
    out.write_text(json.dumps(ref, indent=1) + "\n")
    print(f"wrote {out} ({len(ref['clips'])} clips)")
    for k, v in ref["clips"].items():
        print(k, v)


if __name__ == "__main__":
    main()
