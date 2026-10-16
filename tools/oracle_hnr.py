#!/usr/bin/env python3
"""Offline oracle for HNR on the real speech clips in crates/voice-core/tests/data/speech/.

Development tool only (see tools/oracle.py). Reference: Praat's mean harmonics-to-noise
ratio of the voiced frames, "To Harmonicity (cc)" with a 0.01 s time step, minimum pitch
75 Hz, silence threshold 0.1 and 1.0 periods per window (Praat leaves unvoiced frames
out of the mean).

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_hnr.py [out.json]
"""
import json
import sys
from pathlib import Path

DATA = Path("crates/voice-core/tests/data")


def main():
    import parselmouth
    from parselmouth.praat import call

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DATA / "speech_hnr_oracle.json"
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "time_step_s": 0.01,
            "min_pitch_hz": 75,
            "silence_threshold": 0.1,
            "periods_per_window": 1.0,
            "note": "hnr_db is the mean over voiced frames; null when Praat finds none",
        },
        "clips": {},
    }
    for wav in sorted((DATA / "speech").glob("*.wav")):
        snd = parselmouth.Sound(str(wav))
        h = call(snd, "To Harmonicity (cc)", 0.01, 75, 0.1, 1.0)
        mean = call(h, "Get mean", 0, 0)
        if mean != mean:
            mean = None
        ref["clips"][wav.stem] = {"hnr_db": None if mean is None else round(float(mean), 3)}
    out.write_text(json.dumps(ref, indent=1) + "\n")
    print(f"wrote {out} ({len(ref['clips'])} clips)")
    for k, v in ref["clips"].items():
        print(k, v)


if __name__ == "__main__":
    main()
