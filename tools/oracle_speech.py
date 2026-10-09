#!/usr/bin/env python3
"""Offline oracle for real speech: frame-by-frame Praat f0 tracks for the clips in
crates/voice-core/tests/data/speech/ (see PROVENANCE.md for their source).

Development tool only (see tools/oracle.py). Real speech has no known true f0, so
the reference is Praat's own track (to_pitch, 10 ms step, 75 to 600 Hz).

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_speech.py [out.json]
"""
import json
import sys
from pathlib import Path

DATA = Path("crates/voice-core/tests/data")


def main():
    import parselmouth

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DATA / "speech_pitch_oracle.json"
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "pitch_floor_hz": 75,
            "pitch_ceiling_hz": 600,
            "time_step_s": 0.01,
            "note": "f0 0.0 = unvoiced in Praat",
        },
        "clips": {},
    }
    for wav in sorted((DATA / "speech").glob("*.wav")):
        snd = parselmouth.Sound(str(wav))
        p = snd.to_pitch(time_step=0.01, pitch_floor=75, pitch_ceiling=600)
        ref["clips"][wav.stem] = {
            "times_s": [round(float(t), 6) for t in p.xs()],
            "praat_hz": [round(float(v), 4) for v in p.selected_array["frequency"]],
        }
    out.write_text(json.dumps(ref) + "\n")
    print(f"wrote {out} ({len(ref['clips'])} clips)")


if __name__ == "__main__":
    main()
