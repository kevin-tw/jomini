use crate::{Scalar, Scalar1252, ScalarUtf8};

pub trait Encoding<'a> {
    type ReturnScalar: Scalar<'a>;
    fn scalar(&self, data: &'a [u8]) -> Self::ReturnScalar;
}

#[derive(Debug, Default)]
pub struct Windows1252;

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

#[derive(Debug, Default)]
pub struct Utf8;

impl Utf8 {
    pub fn new() -> Self {
        Utf8
    }
}

impl<'a> Encoding<'a> for Utf8 {
    type ReturnScalar = ScalarUtf8<'a>;
    fn scalar(&self, data: &'a [u8]) -> Self::ReturnScalar {
        ScalarUtf8::new(data)
    }
}
