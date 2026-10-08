#!/usr/bin/env python3
"""Offline test oracle: emit Praat reference values (via Parselmouth) as JSON.

Praat and Parselmouth are GPL-3. This script is a development tool only: it is
never built into, linked with or published alongside voice-core, and no Praat
code is copied. Run it by hand, commit the JSON it produces, and have the Rust
tests compare against that JSON.

Setup (not needed by CI):  pip install praat-parselmouth
Usage: python3 tools/oracle.py fixtures/synthetic [out.json]
"""
import json
import sys
from pathlib import Path


def measure(path):
    import parselmouth
    from parselmouth.praat import call

    snd = parselmouth.Sound(str(path))
    pitch = snd.to_pitch(time_step=0.01, pitch_floor=75, pitch_ceiling=600)
    f0 = pitch.selected_array["frequency"]
    voiced = f0[f0 > 0]
    intensity = snd.to_intensity(minimum_pitch=75, time_step=0.01)
    result = {
        "f0_median_hz": float(sorted(voiced)[len(voiced) // 2]) if len(voiced) else None,
        "voiced_fraction": float(len(voiced) / len(f0)) if len(f0) else 0.0,
        "intensity_mean_db": float(call(intensity, "Get mean", 0, 0, "energy")),
    }
    if snd.sampling_frequency != 16000:
        # Praat's own resampling to 16 kHz, as the reference for our resampler.
        v = snd.resample(16000).values[0]
        trim = 2048  # skip filter edge effects at both ends
        mid = v[trim:-trim]
        result["sample_rate_hz"] = float(snd.sampling_frequency)
        result["resampled_16k_rms_dbfs"] = float(10 * __import__("math").log10(float((mid**2).mean())))
    if len(voiced) > 10:
        pp = call(snd, "To PointProcess (periodic, cc)", 75, 600)
        result["jitter_local"] = float(call(pp, "Get jitter (local)", 0, 0, 0.0001, 0.02, 1.3))
        result["shimmer_local"] = float(call([snd, pp], "Get shimmer (local)", 0, 0, 0.0001, 0.02, 1.3, 1.6))
        harm = call(snd, "To Harmonicity (cc)", 0.01, 75, 0.1, 1.0)
        result["hnr_db"] = float(call(harm, "Get mean", 0, 0))
    return result


def main():
    src = Path(sys.argv[1])
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else src / "oracle.json"
    import parselmouth

    ref = {
        "_meta": {
            "parselmouth": parselmouth.VERSION,
            "praat": parselmouth.PRAAT_VERSION,
            "pitch_floor_hz": 75,
            "pitch_ceiling_hz": 600,
            "intensity_ref_db_offset": 20 * __import__("math").log10(1 / 2e-5),
            "note": "intensity_mean_db is dB SPL (ref 2e-5); subtract intensity_ref_db_offset to get dBFS",
        },
        "clips": {p.stem: measure(p) for p in sorted(src.glob("*.wav"))},
    }
    out.write_text(json.dumps(ref, indent=2) + "\n")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
