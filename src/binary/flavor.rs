use crate::{util::le_i32, Encoding, Scalar1252, Windows1252};

/// Trait customizing decoding values from binary data
pub trait BinaryFlavor<'a>: Sized + Encoding<'a> {
    /// Decode a f32 from 4 bytes of data
    fn visit_f32_1(&self, data: &[u8]) -> f32;

    /// Decode a f32 from 8 bytes of data
    fn visit_f32_2(&self, data: &[u8]) -> f32;
}

/// The default binary flavor
#[derive(Debug)]
pub struct DefaultFlavor(Windows1252);

impl Default for DefaultFlavor {
    fn default() -> Self {
        DefaultFlavor::new()
    }
}

impl DefaultFlavor {
    pub fn new() -> Self {
        DefaultFlavor(Windows1252::new())
    }
}

impl<'a> Encoding<'a> for DefaultFlavor {
    type ReturnScalar = Scalar1252<'a>;

    fn scalar(&self, data: &'a [u8]) -> Self::ReturnScalar {
        self.0.scalar(data)
    }
}

impl<'a> BinaryFlavor<'a> for DefaultFlavor {
    fn visit_f32_1(&self, data: &[u8]) -> f32 {
        // First encoding is an i32 that has a fixed point offset of 3 decimal digits
        (le_i32(data) as f32) / 1000.0
    }

    fn visit_f32_2(&self, data: &[u8]) -> f32 {
        // Second encoding is Q17.15 with 5 fractional digits
        // https://en.wikipedia.org/wiki/Q_(number_format)
        let val = le_i32(data) as f32 / 32768.0;
        (val * 10_0000.0).floor() / 10_0000.0
    }
}
