#!/usr/bin/env python3
"""Offline oracle for speech/silence segmentation: Praat's silence detection on the
clips in crates/voice-core/tests/data/speech/ (see PROVENANCE.md for their source).

Development tool only (see tools/oracle.py). The reference is Praat's
"To TextGrid (silences)" with its default settings (minimum pitch 100 Hz, silence
threshold -25 dB, minimum silent interval 0.1 s, minimum sounding interval 0.1 s).

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_pauses.py [out.json]
"""
import json
import sys
from pathlib import Path

DATA = Path("crates/voice-core/tests/data")
MIN_PITCH = 100.0
THRESHOLD_DB = -25.0
MIN_SILENT_S = 0.1
MIN_SOUNDING_S = 0.1


def main():
    import parselmouth
    from parselmouth.praat import call

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DATA / "speech_pauses_oracle.json"
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "minimum_pitch_hz": MIN_PITCH,
            "silence_threshold_db": THRESHOLD_DB,
            "min_silent_s": MIN_SILENT_S,
            "min_sounding_s": MIN_SOUNDING_S,
            "note": "sounding = [start_s, end_s] intervals labelled 'sounding'",
        },
        "clips": {},
    }
    for wav in sorted((DATA / "speech").glob("*.wav")):
        snd = parselmouth.Sound(str(wav))
        grid = call(
            snd, "To TextGrid (silences)", MIN_PITCH, 0.0, THRESHOLD_DB,
            MIN_SILENT_S, MIN_SOUNDING_S, "silent", "sounding",
        )
        n = call(grid, "Get number of intervals", 1)
        sounding = []
        for i in range(1, n + 1):
            if call(grid, "Get label of interval", 1, i) == "sounding":
                sounding.append([
                    round(call(grid, "Get start time of interval", 1, i), 4),
                    round(call(grid, "Get end time of interval", 1, i), 4),
                ])
        ref["clips"][wav.stem] = {"duration_s": round(snd.duration, 4), "sounding": sounding}
    out.write_text(json.dumps(ref, indent=1) + "\n")
    print(f"wrote {out} ({len(ref['clips'])} clips)")


if __name__ == "__main__":
    main()
