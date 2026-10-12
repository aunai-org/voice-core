//! Speed of the analysis stages on 10 s of synthetic speech-like audio at 16 kHz.
//!
//! Run with `cargo bench -p voice-core`. The real-time factor is the clip length
//! divided by the time taken (higher is faster).

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use voice_core::pitch::{AutocorrEstimator, PitchEstimator};
use voice_core::stream::Analyzer;
use voice_core::{analyze, loudness::integrated_lufs, pace::syllables_per_second};

const RATE: u32 = 16_000;
const SECONDS: usize = 10;

/// A 140 Hz voice with syllable-rate amplitude bursts and a pause every 2 s.
fn clip() -> Vec<f32> {
    (0..RATE as usize * SECONDS)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let burst = (0.5 + 0.5 * (2.0 * core::f32::consts::PI * 4.0 * t).sin()).powi(2);
            let on = if t % 2.0 < 1.6 { 1.0 } else { 0.0 };
            let f0 = 140.0 * (1.0 + 0.05 * (2.0 * core::f32::consts::PI * 0.5 * t).sin());
            let v: f32 = (1..=8)
                .map(|h| (2.0 * core::f32::consts::PI * f0 * h as f32 * t).sin() / h as f32)
                .sum();
            0.2 * burst * on * v
        })
        .collect()
}

fn benches(c: &mut Criterion) {
    let samples = clip();
    let mut g = c.benchmark_group("10 s clip");
    g.throughput(Throughput::Elements(samples.len() as u64));
    g.bench_function("loudness (LUFS)", |b| {
        b.iter(|| integrated_lufs(black_box(&samples), RATE))
    });
    g.bench_function("pitch track", |b| {
        b.iter(|| AutocorrEstimator::default().estimate(black_box(&samples), RATE))
    });
    g.bench_function("syllable rate", |b| {
        b.iter(|| syllables_per_second(black_box(&samples), RATE))
    });
    g.bench_function("full report", |b| b.iter(|| analyze(black_box(&samples), RATE)));
    g.bench_function("streaming analyzer, 10 ms blocks", |b| {
        b.iter(|| {
            let mut a = Analyzer::new(RATE).unwrap();
            let mut n = 0;
            for block in samples.chunks(160) {
                n += a.push(block).len();
            }
            n + a.finish().len()
        })
    });
    g.finish();
}

criterion_group!(analysis, benches);
criterion_main!(analysis);
