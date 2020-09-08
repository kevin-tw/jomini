#[cfg(feature = "derive")]
mod de;
mod tape;

#[cfg(feature = "derive")]
pub use self::de::TextDeserializer;
pub use self::tape::{TextTape, TextToken, text_parser_windows1252};
