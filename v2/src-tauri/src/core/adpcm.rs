//! IMA ADPCM decoder compatible with the Xiaomi Remote 2 Pro ATVV audio stream.
//!
//! This is a faithful port of the V1 Python implementation
//! (`scripts/voice_capture.py::AdpcmDecoder`). The table values and the
//! clamping behaviour are identical so that existing recordings/tests can be
//! compared sample-for-sample.

pub const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

pub const INDEX_TABLE: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

#[derive(Debug, Clone)]
pub struct AdpcmDecoder {
    predictor: i32,
    step_index: i32,
}

impl Default for AdpcmDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl AdpcmDecoder {
    pub fn new() -> Self {
        Self {
            predictor: 0,
            step_index: 0,
        }
    }

    /// Reset to a predictor/step pair received in an ATVV `0x0A` sync packet.
    pub fn reset(&mut self, predictor: i32, step_index: u8) {
        self.predictor = predictor.clamp(-32768, 32767);
        self.step_index = i32::from(step_index).clamp(0, 88);
    }

    pub fn predictor(&self) -> i32 {
        self.predictor
    }

    pub fn step_index(&self) -> i32 {
        self.step_index
    }

    #[inline]
    pub fn decode_nibble(&mut self, nibble: u8) -> i16 {
        let step = STEP_TABLE[self.step_index as usize];
        let mut diff = step >> 3;
        if nibble & 1 != 0 {
            diff += step >> 2;
        }
        if nibble & 2 != 0 {
            diff += step >> 1;
        }
        if nibble & 4 != 0 {
            diff += step;
        }
        if nibble & 8 != 0 {
            self.predictor -= diff;
        } else {
            self.predictor += diff;
        }
        self.predictor = self.predictor.clamp(-32768, 32767);
        self.step_index += INDEX_TABLE[(nibble & 7) as usize];
        self.step_index = self.step_index.clamp(0, 88);
        self.predictor as i16
    }

    pub fn decode_bytes(&mut self, data: &[u8]) -> Vec<i16> {
        let mut samples = Vec::with_capacity(data.len() * 2);
        for &value in data {
            samples.push(self.decode_nibble(value >> 4));
            samples.push(self.decode_nibble(value & 0x0F));
        }
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_reference_sequence() {
        let mut decoder = AdpcmDecoder::new();
        assert_eq!(
            decoder.decode_bytes(&[0x12, 0x34, 0xAB, 0xCD]),
            vec![1, 4, 8, 15, 10, 3, -4, -16]
        );
    }

    #[test]
    fn reset_clamps_inputs() {
        let mut decoder = AdpcmDecoder::new();
        decoder.reset(100_000, 200);
        assert_eq!(decoder.predictor(), 32767);
        assert_eq!(decoder.step_index(), 88);

        decoder.reset(-100_000, 0);
        assert_eq!(decoder.predictor(), -32768);
        assert_eq!(decoder.step_index(), 0);
    }

    #[test]
    fn nibble_decode_matches_python_reference() {
        let mut decoder = AdpcmDecoder::new();
        decoder.reset(0, 0);
        let got: Vec<i16> = (0u8..16).map(|n| decoder.decode_nibble(n)).collect();
        // Generated from the V1 Python decoder (same reset, 0..15).
        assert_eq!(
            got,
            vec![
                0, 1, 4, 8, 15, 27, 47, 88, 82, 66, 41, 10, -28, -84, -181, -380
            ]
        );
    }
}
