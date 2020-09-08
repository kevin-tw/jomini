use crate::{Scalar, Scalar1252};

pub trait Encoding<'a> {
    type ReturnScalar: Scalar<'a>;
    fn scalar(&self, data: &'a [u8]) -> Self::ReturnScalar;
}

pub struct Windows1252<'a>(std::marker::PhantomData<&'a ()>);

impl<'a> Default for Windows1252<'a> {
    fn default() -> Self {
        Windows1252::new()
    }
}

impl<'a> Windows1252<'a> {
    pub fn new() -> Self {
        Windows1252(std::marker::PhantomData)
    }
}

impl<'a> Encoding<'a> for Windows1252<'a> {
    type ReturnScalar = Scalar1252<'a>;
    fn scalar(&self, data: &'a [u8]) -> Self::ReturnScalar {
        Scalar1252::new(data)
    }
}
