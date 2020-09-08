use crate::{Scalar, Scalar1252};

pub trait Encoding<'a> {
    type ReturnScalar: Scalar<'a>;
    fn scalar(&self, data: &'a [u8]) -> Self::ReturnScalar;
}

#[derive(Debug)]
pub struct Windows1252;

impl Default for Windows1252 {
    fn default() -> Self {
        Windows1252::new()
    }
}

impl Windows1252 {
    pub fn new() -> Self {
        Windows1252
    }
}

impl<'a> Encoding<'a> for Windows1252 {
    type ReturnScalar = Scalar1252<'a>;
    fn scalar(&self, data: &'a [u8]) -> Self::ReturnScalar {
        Scalar1252::new(data)
    }
}
