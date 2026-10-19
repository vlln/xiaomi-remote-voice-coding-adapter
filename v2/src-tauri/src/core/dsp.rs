//! Audio pipeline ported from V1.
//!
//! The chain is intentionally identical to `scripts/voice_capture.py`:
//! one-pole high-pass (~220 Hz) -> 3-tap smoothing -> adaptive noise gate ->
//! fixed gain -> 16 kHz -> 48 kHz linear interpolation.

#[derive(Debug, Clone)]
pub struct NoiseReducer {
    prev_input: f64,
    prev_output: f64,
}

impl Default for NoiseReducer {
    fn default() -> Self {
        Self::new()
    }
}

impl NoiseReducer {
    pub fn new() -> Self {
        Self {
            prev_input: 0.0,
            prev_output: 0.0,
        }
    }

    pub fn process(&mut self, samples: &[i16]) -> Vec<i16> {
        let mut high_passed = Vec::with_capacity(samples.len());
        for &sample in samples {
            let output = 0.91 * (self.prev_output + f64::from(sample) - self.prev_input);
            self.prev_input = f64::from(sample);
            self.prev_output = output;
            high_passed.push(output as i32);
        }

        if high_passed.len() < 3 {
            return high_passed.into_iter().map(to_i16).collect();
        }

        let mut result = high_passed.clone();
        for i in 1..high_passed.len() - 1 {
            result[i] = (high_passed[i - 1] + high_passed[i] * 2 + high_passed[i + 1]) >> 2;
        }
        result.into_iter().map(to_i16).collect()
    }
}

#[derive(Debug, Clone)]
pub struct NoiseGate {
    noise_floor: Option<f64>,
    calibration: Vec<f64>,
    gain: f64,
    threshold_db: f64,
    floor_gain: f64,
    attack: f64,
    release: f64,
}

impl Default for NoiseGate {
    fn default() -> Self {
        Self::new()
    }
}

impl NoiseGate {
    pub const CALIBRATION_FRAMES: usize = 30;

    pub fn new() -> Self {
        Self {
            noise_floor: None,
            calibration: Vec::with_capacity(Self::CALIBRATION_FRAMES),
            gain: 1.0,
            threshold_db: 8.0,
            floor_gain: 0.15,
            attack: 0.15,
            release: 0.5,
        }
    }

    pub fn gain(&self) -> f64 {
        self.gain
    }

    pub fn noise_floor(&self) -> Option<f64> {
        self.noise_floor
    }

    pub fn process(&mut self, samples: &[i16]) -> Vec<i16> {
        if samples.is_empty() {
            return Vec::new();
        }
        let n = samples.len();
        let energy = samples
            .iter()
            .map(|&s| {
                let s = f64::from(s);
                s * s
            })
            .sum::<f64>()
            / n as f64;

        if self.calibration.len() < Self::CALIBRATION_FRAMES {
            self.calibration.push(energy);
            if self.calibration.len() == Self::CALIBRATION_FRAMES {
                self.noise_floor = Some(percentile20(&self.calibration).max(1.0));
            }
            return samples.to_vec();
        }

        let mut noise_floor = self.noise_floor.expect("calibrated");
        let threshold = 10f64.powf(self.threshold_db / 20.0);
        let threshold_sq = noise_floor * threshold * threshold;

        if energy < noise_floor {
            noise_floor = (0.9 * noise_floor + 0.1 * energy).max(1.0);
        } else if energy < threshold_sq {
            noise_floor = 0.995 * noise_floor + 0.005 * energy;
        }
        self.noise_floor = Some(noise_floor);

        let target = if energy > threshold_sq {
            1.0
        } else {
            (energy / threshold_sq).max(self.floor_gain)
        };
        let coefficient = if target < self.gain {
            self.attack
        } else {
            self.release
        };
        let new_gain = self.gain * (1.0 - coefficient) + target * coefficient;

        let g0 = self.gain;
        let out = samples
            .iter()
            .enumerate()
            .map(|(i, &sample)| {
                let interpolated = g0 + (new_gain - g0) * (i as f64 / n as f64);
                to_i16((f64::from(sample) * interpolated) as i32)
            })
            .collect();
        self.gain = new_gain;
        out
    }
}

#[derive(Debug, Clone)]
pub struct Resampler {
    previous: i32,
    have_previous: bool,
}

impl Default for Resampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Resampler {
    pub fn new() -> Self {
        Self {
            previous: 0,
            have_previous: false,
        }
    }

    /// 16 kHz -> 48 kHz, three linearly interpolated samples per input sample.
    /// The previous sample is carried across chunks to avoid a discontinuity.
    pub fn resample(&mut self, chunk: &[i16]) -> Vec<i16> {
        if chunk.is_empty() {
            return Vec::new();
        }
        let mut output = Vec::with_capacity(chunk.len() * 3);
        let mut previous = if self.have_previous {
            self.previous
        } else {
            i32::from(chunk[0])
        };

        for &sample in chunk {
            let current = i32::from(sample);
            let delta = current - previous;
            let first = previous + (f64::from(delta) / 3.0).round_ties_even() as i32;
            let second = previous + (f64::from(delta) * 2.0 / 3.0).round_ties_even() as i32;
            output.push(to_i16(first));
            output.push(to_i16(second));
            output.push(to_i16(current));
            previous = current;
        }

        self.previous = previous;
        self.have_previous = true;
        output
    }
}

pub fn apply_gain(samples: &[i16], gain_db: f64) -> Vec<i16> {
    let gain = 10f64.powf(gain_db / 20.0);
    samples
        .iter()
        .map(|&sample| {
            to_i16(
                (f64::from(sample) * gain)
                    .round_ties_even()
                    .clamp(-32768.0, 32767.0) as i32,
            )
        })
        .collect()
}

#[inline]
fn to_i16(value: i32) -> i16 {
    value.clamp(-32768, 32767) as i16
}

fn percentile20(values: &[f64]) -> f64 {
    debug_assert!(!values.is_empty());
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let rank = (sorted.len() - 1) as f64 * 0.2;
    let low = rank.floor() as usize;
    let high = rank.ceil() as usize;
    sorted[low] + (sorted[high] - sorted[low]) * (rank - low as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_reducer_matches_python_reference() {
        let mut reducer = NoiseReducer::new();
        assert_eq!(
            reducer.process(&[100, -200, 300, -400, 500, -600, 700, -800]),
            vec![91, -2, -2, -2, -2, -2, -2, -761]
        );
    }

    #[test]
    fn gate_passes_calibration_and_loud_frame() {
        let mut gate = NoiseGate::new();
        for _ in 0..NoiseGate::CALIBRATION_FRAMES {
            assert_eq!(
                gate.process(&[100, -100, 100, -100]),
                vec![100, -100, 100, -100]
            );
        }
        assert_eq!(gate.gain(), 1.0);
        assert_eq!(
            gate.process(&[1000, -1000, 500, -500, 2000, -2000]),
            vec![1000, -1000, 500, -500, 2000, -2000]
        );
    }

    #[test]
    fn gate_attenuates_quiet_frame_after_calibration() {
        let mut gate = NoiseGate::new();
        for _ in 0..NoiseGate::CALIBRATION_FRAMES {
            gate.process(&[50, -50, 50, -50]);
        }
        let out = gate.process(&[10, -10, 10, -10]);
        assert!(gate.gain() < 0.95);
        assert!(out.iter().all(|s| s.abs() <= 10));
    }

    #[test]
    fn resampler_matches_python_reference() {
        let mut resampler = Resampler::new();
        assert_eq!(
            resampler.resample(&[0, 300, -300, 1000]),
            vec![0, 0, 0, 100, 200, 300, 100, -100, -300, 133, 567, 1000]
        );
        assert_eq!(
            resampler.resample(&[1000, -1000]),
            vec![1000, 1000, 1000, 333, -333, -1000]
        );
    }

    #[test]
    fn apply_gain_matches_python_reference() {
        assert_eq!(
            apply_gain(&[100, -100, 32767, -32768], 15.0),
            vec![562, -562, 32767, -32768]
        );
    }
}
