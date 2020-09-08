pub trait Encoding<'a> {
    type ReturnScalar;
    fn scalar(&self, data: &'a [u8]) -> Self::ReturnScalar;
}