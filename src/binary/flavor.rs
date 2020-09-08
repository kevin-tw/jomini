use crate::{util::le_i32, Encoding, Scalar1252};

/// Trait customizing decoding values from binary data
pub trait BinaryFlavor<'a>: Sized + Encoding<'a> {
    /// Decode a f32 from 4 bytes of data
    fn visit_f32_1(&self, data: &[u8]) -> f32;

    /// Decode a f32 from 8 bytes of data
    fn visit_f32_2(&self, data: &[u8]) -> f32;
}

/// The default binary flavor
#[derive(Debug)]
pub struct DefaultFlavor<'a>(std::marker::PhantomData<&'a ()>);

impl<'a> Default for DefaultFlavor<'a> {
    fn default() -> Self {
        DefaultFlavor::new()
    }
}

impl<'a> DefaultFlavor<'a> {
    pub fn new() -> Self {
        DefaultFlavor(std::marker::PhantomData)
    }
}

impl<'a> Encoding<'a> for DefaultFlavor<'a> {
    type ReturnScalar = Scalar1252<'a>;

    fn scalar(&self, data: &'a [u8]) -> Self::ReturnScalar {
        Scalar1252::new(data)
    }
}

impl<'a> BinaryFlavor<'a> for DefaultFlavor<'a> {
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
