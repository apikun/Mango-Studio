// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Fast Fourier Transform (radix-2 Cooley-Tukey) for bandlimited wavetable mipmap precomputation.
//!
//! Provides mathematically exact harmonic filtering of 2,048-sample single-cycle waveforms so that
//! playback at any frequency up to Nyquist has zero digital aliasing.

use std::f32::consts::PI;

/// In-place radix-2 Cooley-Tukey FFT. `real` and `imag` must have equal lengths that are powers of 2.
pub fn fft(real: &mut [f32], imag: &mut [f32]) {
    transform(real, imag, false);
}

/// In-place radix-2 Cooley-Tukey Inverse FFT with 1/N scaling.
pub fn ifft(real: &mut [f32], imag: &mut [f32]) {
    transform(real, imag, true);
    let scale = 1.0 / real.len() as f32;
    for (r, i) in real.iter_mut().zip(imag.iter_mut()) {
        *r *= scale;
        *i *= scale;
    }
}

fn transform(real: &mut [f32], imag: &mut [f32], inverse: bool) {
    let n = real.len();
    assert!(n.is_power_of_two() && n == imag.len(), "length must be a power of two");

    // Bit-reversal permutation
    let mut j = 0;
    for i in 0..n {
        if i < j {
            real.swap(i, j);
            imag.swap(i, j);
        }
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
    }

    // Cooley-Tukey butterfly stages
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let angle = sign * 2.0 * PI / len as f32;
        let w_step_r = angle.cos();
        let w_step_i = angle.sin();

        let mut i = 0;
        while i < n {
            let mut w_r = 1.0;
            let mut w_i = 0.0;
            for k in 0..half {
                let u_r = real[i + k];
                let u_i = imag[i + k];

                let v_r = real[i + k + half] * w_r - imag[i + k + half] * w_i;
                let v_i = real[i + k + half] * w_i + imag[i + k + half] * w_r;

                real[i + k] = u_r + v_r;
                imag[i + k] = u_i + v_i;
                real[i + k + half] = u_r - v_r;
                imag[i + k + half] = u_i - v_i;

                let next_w_r = w_r * w_step_r - w_i * w_step_i;
                let next_w_i = w_r * w_step_i + w_i * w_step_r;
                w_r = next_w_r;
                w_i = next_w_i;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// Bandlimits a real-valued signal of length N (e.g. 2,048 samples) by removing all frequency components
/// above `max_harmonic`.
pub fn bandlimit(signal: &[f32], max_harmonic: usize) -> Vec<f32> {
    let n = signal.len();
    assert!(n.is_power_of_two(), "signal length must be a power of two");

    let mut real = signal.to_vec();
    let mut imag = vec![0.0; n];

    fft(&mut real, &mut imag);

    // Bins 0 is DC. Bins 1..n/2 are positive frequencies (harmonics 1..n/2).
    // Bins n/2..n are negative frequencies corresponding to n - k.
    let nyquist = n / 2;
    let cutoff = max_harmonic.min(nyquist);

    // Zero out all harmonics strictly above `cutoff`
    for k in (cutoff + 1)..=nyquist {
        real[k] = 0.0;
        imag[k] = 0.0;
        if k < nyquist {
            let conjugate = n - k;
            real[conjugate] = 0.0;
            imag[conjugate] = 0.0;
        }
    }

    if cutoff == 0 {
        real[0] = 0.0;
        imag[0] = 0.0;
    }

    ifft(&mut real, &mut imag);
    real
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_ifft_round_trip() {
        let original: Vec<f32> = (0..2048).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut real = original.clone();
        let mut imag = vec![0.0; 2048];

        fft(&mut real, &mut imag);
        ifft(&mut real, &mut imag);

        for (a, b) in original.iter().zip(real.iter()) {
            assert!((a - b).abs() < 1e-4, "round-trip error: {a} vs {b}");
        }
    }

    #[test]
    fn bandlimiting_filters_higher_harmonics() {
        // Construct composite signal: fundamental (bin 1) + high frequency (bin 50)
        let n = 2048;
        let mut signal = vec![0.0; n];
        for i in 0..n {
            let phi1 = i as f32 * 2.0 * PI / n as f32;
            let phi50 = i as f32 * 50.0 * 2.0 * PI / n as f32;
            signal[i] = phi1.sin() + 0.5 * phi50.sin();
        }

        // Bandlimit to 10 harmonics (should completely remove bin 50)
        let filtered = bandlimit(&signal, 10);

        // Check that filtered signal is virtually identical to pure fundamental
        for i in 0..n {
            let phi1 = i as f32 * 2.0 * PI / n as f32;
            let expected = phi1.sin();
            assert!((filtered[i] - expected).abs() < 1e-3, "bin 50 was not filtered out: {} vs {}", filtered[i], expected);
        }
    }
}
