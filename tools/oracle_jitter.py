#!/usr/bin/env python3
"""Offline oracle for jitter on the real speech clips in crates/voice-core/tests/data/speech/.

Development tool only (see tools/oracle.py). Reference: Praat's local jitter on the
pulses from "To PointProcess (periodic, cc)" (75 to 600 Hz), with the usual settings
(shortest period 0.1 ms, longest 20 ms, maximum period factor 1.3).

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_jitter.py [out.json]
"""
import json
import sys
from pathlib import Path

DATA = Path("crates/voice-core/tests/data")


def main():
    import parselmouth
    from parselmouth.praat import call

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DATA / "speech_jitter_oracle.json"
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "pitch_floor_hz": 75,
            "pitch_ceiling_hz": 600,
            "shortest_period_s": 0.0001,
            "longest_period_s": 0.02,
            "max_period_factor": 1.3,
            "note": "jitter_local is a fraction (0.01 = 1%); null when Praat finds too few periods",
        },
        "clips": {},
    }
    for wav in sorted((DATA / "speech").glob("*.wav")):
        snd = parselmouth.Sound(str(wav))
        pp = call(snd, "To PointProcess (periodic, cc)", 75, 600)
        n_pulses = call(pp, "Get number of points")
        try:
            jit = call(pp, "Get jitter (local)", 0, 0, 0.0001, 0.02, 1.3)
        except Exception:
            jit = None
        if jit is not None and jit != jit:
            jit = None
        ref["clips"][wav.stem] = {
            "pulses": int(n_pulses),
            "jitter_local": None if jit is None else round(float(jit), 6),
        }
    out.write_text(json.dumps(ref, indent=1) + "\n")
    print(f"wrote {out} ({len(ref['clips'])} clips)")
    for k, v in ref["clips"].items():
        print(k, v)


if __name__ == "__main__":
    main()
