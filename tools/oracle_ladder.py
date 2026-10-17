#!/usr/bin/env python3
"""Offline oracle for the level ladder on the real speech clips in
crates/voice-core/tests/data/speech/.

Development tool only (see tools/oracle.py). Each clip is cut into three equal parts
that play the three steps of a volume exercise, with gains of 0, +10 and -8 dB applied
to the parts (the Rust test applies the same gains). For each part the reference is
Praat's "To TextGrid (silences)" run on the part alone (minimum pitch 100 Hz, threshold
-25 dB, 0.1 s minimum intervals), and the energy mean of the samples inside the
'sounding' intervals from Praat's "Get root-mean-square", as 10 log10 of mean square.

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_ladder.py [out.json]
"""
import json
import math
import sys
from pathlib import Path

DATA = Path("crates/voice-core/tests/data")
GAINS_DB = [0.0, 10.0, -8.0]
MIN_SPEECH_S = 0.2


def main():
    import parselmouth
    from parselmouth.praat import call

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DATA / "speech_ladder_oracle.json"
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "gains_db": GAINS_DB,
            "note": "level_dbfs per third of the clip after the gain; null with under 0.2 s of sounding",
        },
        "clips": {},
    }
    for wav in sorted((DATA / "speech").glob("*.wav")):
        snd = parselmouth.Sound(str(wav))
        third = snd.duration / 3
        levels = []
        for k, gain in enumerate(GAINS_DB):
            part = call(snd, "Extract part", k * third, (k + 1) * third, "rectangular", 1.0, "no")
            call(part, "Multiply", 10 ** (gain / 20))
            grid = call(part, "To TextGrid (silences)", 100.0, 0.0, -25.0, 0.1, 0.1, "silent", "sounding")
            energy, dur = 0.0, 0.0
            for i in range(1, call(grid, "Get number of intervals", 1) + 1):
                if call(grid, "Get label of interval", 1, i) != "sounding":
                    continue
                t1 = call(grid, "Get start time of interval", 1, i)
                t2 = call(grid, "Get end time of interval", 1, i)
                rms = call(part, "Get root-mean-square", t1, t2)
                energy += rms * rms * (t2 - t1)
                dur += t2 - t1
            levels.append(round(10 * math.log10(energy / dur), 3) if dur >= MIN_SPEECH_S else None)
        ref["clips"][wav.stem] = {"level_dbfs": levels}
    out.write_text(json.dumps(ref, indent=1) + "\n")
    print(f"wrote {out} ({len(ref['clips'])} clips)")
    for k, v in ref["clips"].items():
        print(k, v["level_dbfs"])


if __name__ == "__main__":
    main()
