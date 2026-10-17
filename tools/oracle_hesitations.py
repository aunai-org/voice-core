#!/usr/bin/env python3
"""Offline oracle for hesitation events on the real speech clips in
crates/voice-core/tests/data/speech/.

Development tool only (see tools/oracle.py). The same two rules as the Rust module are
applied to Praat's tracks:
  - pauses: silent intervals of "To TextGrid (silences)" (100 Hz, -25 dB, 0.1 s minimum
    intervals) that lie between two sounding intervals and last at least 0.5 s;
  - held sounds: voiced stretches of Praat's pitch track ("To Pitch (cc)", 75 to 600 Hz,
    0.01 s step, gaps of up to 2 unvoiced frames bridged) that last at least 0.4 s with a
    pitch standard deviation of at most 1 semitone around their mean and a level range of
    at most 8 dB (Praat intensity, minimum pitch 100 Hz, 0.01 s step, largest minus
    smallest value, leaving out the first and last 30 ms).
Each clip lists its events as [kind, start_s, end_s].

Setup (not needed by CI):  pip install praat-parselmouth numpy
Usage: python3 tools/oracle_hesitations.py [out.json]
"""
import json
import math
import sys
from pathlib import Path

DATA = Path("crates/voice-core/tests/data")
MIN_PAUSE_S = 0.5
MIN_HELD_S = 0.4
MAX_HELD_SD = 1.0
MAX_HELD_RANGE_DB = 8.0
EDGE_S = 0.03
MAX_GAP = 2
HOP = 0.01


def stretches(f0):
    out, cur, gap = [], None, 0
    for i, v in enumerate(f0):
        if v > 0:
            cur = (cur[0], i) if cur else (i, i)
            gap = 0
        elif cur:
            gap += 1
            if gap > MAX_GAP:
                out.append(cur)
                cur = None
    if cur:
        out.append(cur)
    return out


def sd_semitones(hz):
    if len(hz) < 2:
        return 0.0
    mean = sum(hz) / len(hz)
    st = [12 * math.log2(h / mean) for h in hz]
    m = sum(st) / len(st)
    return math.sqrt(sum((x - m) ** 2 for x in st) / len(st))


def main():
    import parselmouth
    from parselmouth.praat import call

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DATA / "speech_hesitations_oracle.json"
    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "min_pause_s": MIN_PAUSE_S,
            "min_held_s": MIN_HELD_S,
            "max_held_sd_semitones": MAX_HELD_SD,
            "max_held_level_range_db": MAX_HELD_RANGE_DB,
            "note": "events as [kind, start_s, end_s], kind 'pause' or 'held'",
        },
        "clips": {},
    }
    for wav in sorted((DATA / "speech").glob("*.wav")):
        snd = parselmouth.Sound(str(wav))
        events = []
        grid = call(snd, "To TextGrid (silences)", 100.0, 0.0, -25.0, 0.1, 0.1, "silent", "sounding")
        n = call(grid, "Get number of intervals", 1)
        for i in range(2, n):  # interior intervals only
            if call(grid, "Get label of interval", 1, i) != "silent":
                continue
            t1 = call(grid, "Get start time of interval", 1, i)
            t2 = call(grid, "Get end time of interval", 1, i)
            if t2 - t1 >= MIN_PAUSE_S:
                events.append(["pause", round(t1, 3), round(t2, 3)])
        p = snd.to_pitch(time_step=HOP, pitch_floor=75, pitch_ceiling=600)
        f0 = p.selected_array["frequency"]
        xs = p.xs()
        inten = snd.to_intensity(minimum_pitch=100, time_step=HOP)
        ix, iv = inten.xs(), inten.values[0]
        for a, b in stretches(f0):
            start, end = xs[a] - HOP / 2, xs[b] + HOP / 2
            hz = [v for v in f0[a : b + 1] if v > 0]
            m = (ix >= start + EDGE_S) & (ix <= end - EDGE_S)
            rng = float(iv[m].max() - iv[m].min()) if m.any() else 0.0
            if end - start >= MIN_HELD_S and sd_semitones(hz) <= MAX_HELD_SD and rng <= MAX_HELD_RANGE_DB:
                events.append(["held", round(float(max(start, 0.0)), 3), round(float(end), 3)])
        events.sort(key=lambda e: e[1])
        ref["clips"][wav.stem] = {"events": events}
    out.write_text(json.dumps(ref, indent=1) + "\n")
    total = sum(len(c["events"]) for c in ref["clips"].values())
    print(f"wrote {out} ({len(ref['clips'])} clips, {total} events)")
    for k, c in ref["clips"].items():
        print(k, c["events"])


if __name__ == "__main__":
    main()
