#!/usr/bin/env python3
"""Offline oracle for syllable nuclei on the real clips in
crates/voice-core/tests/data/speech/ (see PROVENANCE.md for their source).

Development tool only (see tools/oracle.py). Uses the same Praat-intensity-plus-
de Jong & Wempe (2009) peak rules as tools/oracle_pace.py. The reference is a
Praat-based nucleus detector, not a human syllable count.

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_speech_pace.py [out.json]
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle_pace import nuclei  # noqa: E402

DATA = Path("crates/voice-core/tests/data")


def main():
    import parselmouth

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DATA / "speech_pace_oracle.json"
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "note": "Praat intensity (min pitch 50 Hz, 10 ms step) and pitch (75-600 Hz); 99th percentile - 25 dB, dip >= 2 dB, voiced. Reference is this detector, not a human count.",
        },
        "clips": {},
    }
    for wav in sorted((DATA / "speech").glob("*.wav")):
        snd = parselmouth.Sound(str(wav))
        x = snd.values[0]
        if snd.sampling_frequency != 16000:
            raise SystemExit(f"{wav}: expected 16 kHz")
        n = nuclei(x)
        ref["clips"][wav.stem] = {"duration_s": round(snd.duration, 4), "praat_times_s": n}
    out.write_text(json.dumps(ref, indent=1) + "\n")
    print(f"wrote {out} ({len(ref['clips'])} clips)")


if __name__ == "__main__":
    main()
