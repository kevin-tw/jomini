use crate::util::{contains_zero_byte, le_u64, repeat_byte};
use crate::{
    data::{is_boundary, is_whitespace},
    Encoding, Scalar1252, ScalarUtf8, Utf8, Windows1252,
};
use crate::{Error, ErrorKind, Rgb, Scalar};

/// High level structure for parsing plain text data
pub struct TextParser;

impl TextParser {
    /// Creates a parser tailored to the given encoding
    pub fn from_encoding<'a, E>(encoding: E) -> TextTapeParser<E>
    where
        E: Encoding<'a>,
    {
        TextTapeParser::with_encoding(encoding)
    }

    /// Creates a parser for ingesting uft-8 encoded data
    pub fn utf8_parser() -> TextTapeParser<Utf8> {
        TextTapeParser::with_encoding(Utf8::new())
    }

    /// Parses utf-8 encoded data into a tape
    pub fn from_utf8(data: &[u8]) -> Result<TextTape<ScalarUtf8>, Error> {
        Self::utf8_parser().parse_slice(data)
    }

    /// Creates a parser for ingesting windows-1252 encoded data
    pub fn windows1252_parser() -> TextTapeParser<Windows1252> {
        TextTapeParser::with_encoding(Windows1252::new())
    }

    /// Parses windows-1252 encoded data into a tape
    pub fn from_windows1252(data: &[u8]) -> Result<TextTape<Scalar1252>, Error> {
        Self::windows1252_parser().parse_slice(data)
    }
}

/// Represents a valid text value
#[derive(Debug, PartialEq)]
pub enum TextToken<S> {
    /// Index of the `TextToken::End` that signifies this array's termination
    Array(usize),

    /// Index of the `TextToken::End` that signifies this array's termination
    Object(usize),

    /// Extracted scalar value
    Scalar(S),

    /// Index of the start of this object
    End(usize),

    /// Represents a text encoded rgb value
    Rgb(Box<Rgb>),
}

/// Creates a parser that a writes to a text tape
#[derive(Debug, Default)]
pub struct TextTapeParser<F> {
    encoding: F,
}

impl<'a, F> TextTapeParser<F>
where
    F: Encoding<'a>,
{
    /// Create a binary parser with a given flavor
    pub fn with_encoding(flavor: F) -> Self {
        TextTapeParser { encoding: flavor }
    }

    /// Parse the text format and return the data tape
    pub fn parse_slice(self, data: &'a [u8]) -> Result<TextTape<F::ReturnScalar>, Error> {
        let toks = Vec::new();
        let mut res = TextTape { token_tape: toks };
        self.parse_slice_into_tape(data, &mut res)?;
        Ok(res)
    }

    /// Parse the text format into the given tape.
    pub fn parse_slice_into_tape(
        self,
        data: &'a [u8],
        tape: &mut TextTape<F::ReturnScalar>,
    ) -> Result<(), Error> {
        let token_tape = &mut tape.token_tape;
        token_tape.clear();
        token_tape.reserve(data.len() / 100 * 15);
        let mut state = ParserState {
            data,
            original_length: data.len(),
            token_tape,
            flavor: self.encoding,
        };

        state.parse()?;
        Ok(())
    }
}

struct ParserState<'a, 'b, F, S> {
    data: &'a [u8],
    original_length: usize,
    flavor: F,
    token_tape: &'b mut Vec<TextToken<S>>,
}

/// Houses the tape of tokens that is extracted from plaintext data
#[derive(Debug, Default)]
pub struct TextTape<S> {
    token_tape: Vec<TextToken<S>>,
}

#[derive(Debug, PartialEq)]
enum ParseState {
    Key,
    KeyValueSeparator,
    ObjectValue,
    ArrayValue,
    ParseOpen,
    FirstValue,
    EmptyObject,
    RgbOpen,
    RgbR,
    RgbG,
    RgbB,
    RgbClose,
}

impl<'a, S> TextTape<S>
where
    S: Scalar<'a>,
{
    // Creates a new text tape
    pub fn new() -> Self {
        TextTape {
            token_tape: Vec::new(),
        }
    }

    /// Return the parsed tokens
    pub fn tokens(&self) -> &[TextToken<S>] {
        self.token_tape.as_slice()
    }
}

impl<'a, 'b, F, S> ParserState<'a, 'b, F, S>
where
    F: Encoding<'a, ReturnScalar = S>,
    S: Scalar<'a>,
{
    fn offset(&self, data: &[u8]) -> usize {
        self.original_length - data.len()
    }

    /// Skips whitespace that may terminate the file
    #[inline]
    fn skip_ws_t(&mut self, mut data: &'a [u8]) -> &'a [u8] {
        loop {
            let start_ptr = data.as_ptr();
            let end_ptr = unsafe { start_ptr.add(data.len()) };

            let nind = unsafe { forward_search(start_ptr, end_ptr, |x| !is_whitespace(x)) };
            let ind = nind.unwrap_or_else(|| data.len());
            let (_, rest) = data.split_at(ind);
            data = rest;

            if data.get(0).map_or(false, |x| *x == b'#') {
                if let Some(idx) = data.iter().position(|&x| x == b'\n') {
                    data = &data[idx..];
                } else {
                    return &[];
                }
            } else {
                return data;
            }
        }
    }

    #[inline]
    fn split_at_scalar(&self, d: &'a [u8]) -> (F::ReturnScalar, &'a [u8]) {
        let start_ptr = d.as_ptr();
        let end_ptr = unsafe { start_ptr.add(d.len()) };

        let nind = unsafe { forward_search(start_ptr, end_ptr, is_boundary) };
        let mut ind = nind.unwrap_or_else(|| d.len());

        // To work with cases where we have "==bar" we ensure that found index is at least one
        ind = std::cmp::max(ind, 1);
        let (scalar, rest) = d.split_at(ind);
        (self.flavor.scalar(scalar), rest)
    }

    /// I'm not smart enough to figure out the behavior of handling escape sequences when
    /// when scanning multi-bytes, so this fallback is for when I was to reset and
    /// process bytewise. It is much slower, but escaped strings should be rare enough
    /// that this shouldn't be an issue
    fn parse_quote_scalar_fallback(
        &self,
        d: &'a [u8],
    ) -> Result<(F::ReturnScalar, &'a [u8]), Error> {
        let mut pos = 1;
        while pos < d.len() {
            if d[pos] == b'\\' {
                pos += 2;
            } else if d[pos] == b'"' {
                let scalar = self.flavor.scalar(&d[1..pos]);
                return Ok((scalar, &d[pos + 1..]));
            } else {
                pos += 1;
            }
        }

        Err(Error::eof())
    }

    #[inline]
    fn _parse_quote_scalar(&self, d: &'a [u8]) -> Result<(F::ReturnScalar, &'a [u8]), Error> {
        let sd = &d[1..];
        let mut offset = 0;
        let mut chunk_iter = sd.chunks_exact(8);
        while let Some(n) = chunk_iter.next() {
            let acc = le_u64(n);
            if contains_zero_byte(acc ^ repeat_byte(b'\\')) {
                return self.parse_quote_scalar_fallback(d);
            } else if contains_zero_byte(acc ^ repeat_byte(b'"')) {
                let end_idx = n.iter().position(|&x| x == b'"').unwrap_or(0) + offset;
                let scalar = self.flavor.scalar(&sd[..end_idx]);
                return Ok((scalar, &d[end_idx + 2..]));
            }

            offset += 8;
        }

        let remainder = chunk_iter.remainder();
        let mut pos = 0;
        while pos < remainder.len() {
            if remainder[pos] == b'\\' {
                pos += 2;
            } else if remainder[pos] == b'"' {
                let end_idx = pos + offset;
                let scalar = self.flavor.scalar(&sd[..end_idx]);
                return Ok((scalar, &d[end_idx + 2..]));
            } else {
                pos += 1;
            }
        }

        Err(Error::eof())
    }

    #[inline]
    fn parse_quote_scalar(&mut self, d: &'a [u8]) -> Result<&'a [u8], Error> {
        let (scalar, data) = self._parse_quote_scalar(d)?;
        self.token_tape.push(TextToken::Scalar(scalar));
        Ok(data)
    }

    #[inline]
    fn parse_scalar(&mut self, d: &'a [u8]) -> &'a [u8] {
        let (scalar, rest) = self.split_at_scalar(d);
        self.token_tape.push(TextToken::Scalar(scalar));
        rest
    }

    #[inline]
    fn parse_key_value_separator(&mut self, d: &'a [u8]) -> &'a [u8] {
        // Most key values are separated by an equal sign but there are some fields like
        // map_area_data that does not have a separator.
        //
        // ```
        // map_area_data{
        //   brittany_area={
        //   # ...
        // ```
        //
        // Additionally it's possible for there to be heterogenus objects:
        //
        // ```
        // brittany_area = { color = { 10 10 10 } 100 200 300 }
        // ```
        //
        // These are especially tricky, but essentially this function's job is to skip the equal
        // token (the 99.9% typical case) if possible.
        if d[0] == b'=' {
            &d[1..]
        } else {
            d
        }
    }

    /// Clear previously parsed data and parse the given data
    #[inline]
    pub fn parse(&mut self) -> Result<(), Error> {
        let mut data = self.data;
        let mut state = ParseState::Key;
        let mut red = 0;
        let mut green = 0;
        let mut blue = 0;

        // This variable keeps track of outer array when we're parsing a hidden object.
        // A hidden object textually looks like:
        //     levels={ 10 0=2 1=2 }
        // which we will translate into
        //     levels={ 10 { 0=2 1=2 } }
        // with the help of this variable. As when we'll only see one END token to signify
        // both the end of the array and object, but we'll produce two TextToken::End.
        let mut array_ind_of_hidden_obj = None;

        let mut parent_ind = 0;
        loop {
            data = self.skip_ws_t(data);
            if data.is_empty() {
                if state == ParseState::RgbOpen {
                    state = ParseState::Key;
                    let scalar = self.flavor.scalar(b"rgb");
                    self.token_tape.push(TextToken::Scalar(scalar));
                }

                if parent_ind == 0 && state == ParseState::Key {
                    return Ok(());
                } else {
                    return Err(Error::eof());
                }
            }

            match state {
                ParseState::EmptyObject => {
                    if data[0] != b'}' {
                        return Err(Error::new(ErrorKind::InvalidEmptyObject {
                            offset: self.offset(data),
                        }));
                    }
                    data = &data[1..];
                    state = ParseState::Key;
                }
                ParseState::Key => {
                    match data[0] {
                        b'}' => {
                            let grand_ind = match self.token_tape.get(parent_ind) {
                                Some(TextToken::Array(x)) => *x,
                                Some(TextToken::Object(x)) => *x,
                                _ => 0,
                            };

                            state = match self.token_tape.get(grand_ind) {
                                Some(TextToken::Array(_x)) => ParseState::ArrayValue,
                                Some(TextToken::Object(_x)) => ParseState::Key,
                                _ => ParseState::Key,
                            };

                            let end_idx = self.token_tape.len();
                            if parent_ind == 0 && grand_ind == 0 {
                                return Err(Error::new(ErrorKind::StackEmpty {
                                    offset: self.offset(data),
                                }));
                            }

                            if let Some(parent) = self.token_tape.get_mut(parent_ind) {
                                *parent = TextToken::Object(end_idx);
                            }
                            self.token_tape.push(TextToken::End(parent_ind));

                            if let Some(array_ind) = array_ind_of_hidden_obj.take() {
                                let end_idx = self.token_tape.len();
                                self.token_tape.push(TextToken::End(array_ind));

                                // Grab the grand parent from the outer array. Even though the logic should
                                // be more strict (ie: throwing an error when if the parent array index doesn't exist,
                                // or if the parent doesn't exist), but since hidden objects are such a rather rare
                                // occurrence, it's better to be flexible
                                let grand_ind =
                                    if let Some(parent) = self.token_tape.get_mut(array_ind) {
                                        let grand_ind = match parent {
                                            TextToken::Array(x) => *x,
                                            _ => 0,
                                        };
                                        *parent = TextToken::Array(end_idx);
                                        grand_ind
                                    } else {
                                        0
                                    };

                                state = match self.token_tape.get(grand_ind) {
                                    Some(TextToken::Array(_x)) => ParseState::ArrayValue,
                                    Some(TextToken::Object(_x)) => ParseState::Key,
                                    _ => ParseState::Key,
                                };
                                parent_ind = grand_ind;
                            } else {
                                parent_ind = grand_ind;
                            }

                            data = &data[1..];
                        }

                        // Empty object! Skip
                        b'{' => {
                            data = &data[1..];
                            state = ParseState::EmptyObject;
                        }

                        b'"' => {
                            data = self.parse_quote_scalar(data)?;
                            state = ParseState::KeyValueSeparator;
                        }
                        _ => {
                            data = self.parse_scalar(data);
                            state = ParseState::KeyValueSeparator;
                        }
                    }
                }
                ParseState::KeyValueSeparator => {
                    data = self.parse_key_value_separator(data);
                    state = ParseState::ObjectValue;
                }
                ParseState::ObjectValue => {
                    match data[0] {
                        b'{' => {
                            self.token_tape.push(TextToken::Array(0));
                            state = ParseState::ParseOpen;
                            data = &data[1..];
                        }

                        // Check to not parse too far into the object's array trailer
                        b'}' => {
                            state = ParseState::Key;
                        }

                        b'"' => {
                            data = self.parse_quote_scalar(data)?;
                            state = ParseState::Key;
                        }
                        b'r' => {
                            let rgb_detected = data.get(1).map_or(false, |&x| x == b'g')
                                && data.get(2).map_or(false, |&x| x == b'b')
                                && data.get(3).map_or(false, |&x| is_boundary(x));
                            if rgb_detected {
                                data = &data[3..];
                                state = ParseState::RgbOpen
                            } else {
                                data = self.parse_scalar(data);
                                state = ParseState::Key;
                            }
                        }
                        _ => {
                            data = self.parse_scalar(data);
                            state = ParseState::Key;
                        }
                    }
                }
                ParseState::ParseOpen => {
                    match data[0] {
                        // Empty array
                        b'}' => {
                            let ind = self.token_tape.len() - 1;
                            state = match self.token_tape.get(parent_ind) {
                                Some(TextToken::Array(_x)) => ParseState::ArrayValue,
                                Some(TextToken::Object(_x)) => ParseState::Key,
                                _ => ParseState::Key,
                            };

                            self.token_tape[ind] = TextToken::Array(ind + 1);
                            self.token_tape.push(TextToken::End(ind));
                            data = &data[1..];
                        }

                        // Array of objects
                        b'{' => {
                            let ind = self.token_tape.len() - 1;
                            self.token_tape[ind] = TextToken::Array(parent_ind);
                            parent_ind = ind;
                            state = ParseState::ArrayValue;
                        }
                        b'"' => {
                            data = self.parse_quote_scalar(data)?;
                            state = ParseState::FirstValue;
                        }
                        _ => {
                            data = self.parse_scalar(data);
                            state = ParseState::FirstValue;
                        }
                    }
                }
                ParseState::FirstValue => match data[0] {
                    b'=' => {
                        let ind = self.token_tape.len() - 2;
                        self.token_tape[ind] = TextToken::Object(parent_ind);
                        data = &data[1..];
                        parent_ind = ind;
                        state = ParseState::ObjectValue;
                    }
                    _ => {
                        let ind = self.token_tape.len() - 2;
                        self.token_tape[ind] = TextToken::Array(parent_ind);
                        parent_ind = ind;
                        state = ParseState::ArrayValue;
                    }
                },
                ParseState::ArrayValue => match data[0] {
                    b'{' => {
                        self.token_tape.push(TextToken::Array(0));
                        state = ParseState::ParseOpen;
                        data = &data[1..];
                    }
                    b'}' => {
                        let grand_ind = match self.token_tape.get(parent_ind) {
                            Some(TextToken::Array(x)) => *x,
                            Some(TextToken::Object(x)) => *x,
                            _ => 0,
                        };

                        state = match self.token_tape.get(grand_ind) {
                            Some(TextToken::Array(_x)) => ParseState::ArrayValue,
                            Some(TextToken::Object(_x)) => ParseState::Key,
                            _ => ParseState::Key,
                        };

                        let end_idx = self.token_tape.len();
                        self.token_tape[parent_ind] = TextToken::Array(end_idx);
                        self.token_tape.push(TextToken::End(parent_ind));
                        parent_ind = grand_ind;
                        data = &data[1..];
                    }
                    b'"' => {
                        data = self.parse_quote_scalar(data)?;
                        state = ParseState::ArrayValue;
                    }
                    b'=' => {
                        // CK3 introduced hidden object inside lists so we work around it by trying to
                        // make the object explicit
                        let hidden_object = TextToken::Object(parent_ind);
                        array_ind_of_hidden_obj = Some(parent_ind);
                        parent_ind = self.token_tape.len() - 1;
                        self.token_tape
                            .insert(self.token_tape.len() - 1, hidden_object);
                        state = ParseState::ObjectValue;
                        data = &data[1..];
                    }
                    _ => {
                        data = self.parse_scalar(data);
                        state = ParseState::ArrayValue;
                    }
                },
                ParseState::RgbOpen => match data[0] {
                    b'{' => {
                        data = &data[1..];
                        state = ParseState::RgbR;
                    }
                    _ => {
                        state = ParseState::Key;
                        let scalar = self.flavor.scalar(b"rgb");
                        self.token_tape.push(TextToken::Scalar(scalar));
                    }
                },
                ParseState::RgbR => {
                    let (r, rest) = self.split_at_scalar(data);
                    if let Ok(x) = r.to_u64() {
                        red = x as u32;
                    } else {
                        return Err(Error::new(ErrorKind::InvalidSyntax {
                            offset: self.offset(data),
                            msg: format!("unable to decode color channel: {}", r.to_utf8()),
                        }));
                    }

                    state = ParseState::RgbG;
                    data = rest;
                }
                ParseState::RgbG => {
                    let (r, rest) = self.split_at_scalar(data);
                    if let Ok(x) = r.to_u64() {
                        green = x as u32;
                    } else {
                        return Err(Error::new(ErrorKind::InvalidSyntax {
                            offset: self.offset(data),
                            msg: format!("unable to decode color channel: {}", r.to_utf8()),
                        }));
                    }

                    state = ParseState::RgbB;
                    data = rest;
                }
                ParseState::RgbB => {
                    let (r, rest) = self.split_at_scalar(data);
                    if let Ok(x) = r.to_u64() {
                        blue = x as u32;
                    } else {
                        return Err(Error::new(ErrorKind::InvalidSyntax {
                            offset: self.offset(data),
                            msg: format!("unable to decode color channel: {}", r.to_utf8()),
                        }));
                    }

                    state = ParseState::RgbClose;
                    data = rest;
                }
                ParseState::RgbClose => match data[0] {
                    b'}' => {
                        self.token_tape.push(TextToken::Rgb(Box::new(Rgb {
                            r: red,
                            b: blue,
                            g: green,
                        })));
                        data = &data[1..];
                        state = ParseState::Key;
                    }
                    _ => {
                        return Err(Error::new(ErrorKind::InvalidSyntax {
                            offset: self.offset(data),
                            msg: "unable to detect rgb close".to_string(),
                        }));
                    }
                },
            }
        }
    }
}

fn sub(a: *const u8, b: *const u8) -> usize {
    debug_assert!(a >= b);
    (a as usize) - (b as usize)
}

#[inline(always)]
unsafe fn forward_search<F: Fn(u8) -> bool>(
    start_ptr: *const u8,
    end_ptr: *const u8,
    confirm: F,
) -> Option<usize> {
    let mut ptr = start_ptr;
    while ptr < end_ptr {
        if confirm(*ptr) {
            return Some(sub(ptr, start_ptr));
        }
        ptr = ptr.offset(1);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scalar1252;

    fn parse<'a>(data: &'a [u8]) -> Result<TextTape<Scalar1252<'a>>, Error> {
        TextParser::from_windows1252(data)
    }

    #[test]
    fn test_simple_event() {
        let data = b"foo=bar";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Scalar(Scalar1252::new(b"bar")),
            ]
        );
    }

    #[test]
    fn test_error_offset() {
        let data = b"foo={}} a=c";
        let err = TextParser::from_windows1252(data).unwrap_err();
        match err.kind() {
            ErrorKind::StackEmpty { offset, .. } => {
                assert_eq!(*offset, 6);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn test_simple_event_with_spaces() {
        let data = b"  \t\t foo =bar \r\ndef=\tqux";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Scalar(Scalar1252::new(b"bar")),
                TextToken::Scalar(Scalar1252::new(b"def")),
                TextToken::Scalar(Scalar1252::new(b"qux")),
            ]
        );
    }

    #[test]
    fn test_scalars_with_quotes() {
        let data = br#""foo"="bar" "3"="1444.11.11""#;
        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Scalar(Scalar1252::new(b"bar")),
                TextToken::Scalar(Scalar1252::new(b"3")),
                TextToken::Scalar(Scalar1252::new(b"1444.11.11")),
            ]
        );
    }

    #[test]
    fn test_escaped_quotes() {
        let data = br#"name = "Joe \"Captain\" Rogers""#;

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"name")),
                TextToken::Scalar(Scalar1252::new(br#"Joe \"Captain\" Rogers"#)),
            ]
        );
    }

    #[test]
    fn test_escaped_quotes_short() {
        let data = br#"name = "J Rogers \"a""#;

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"name")),
                TextToken::Scalar(Scalar1252::new(br#"J Rogers \"a"#)),
            ]
        );
    }

    #[test]
    fn test_escaped_quotes_crazy() {
        let data = br#"custom_name="THE !@#$%^&*( '\"LEGION\"')""#;

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"custom_name")),
                TextToken::Scalar(Scalar1252::new(br#"THE !@#$%^&*( '\"LEGION\"')"#)),
            ]
        );
    }

    #[test]
    fn test_numbers_are_scalars() {
        let data = b"foo=1.000";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Scalar(Scalar1252::new(b"1.000")),
            ]
        );
    }

    #[test]
    fn test_object_event() {
        let data = b"foo={bar=qux}";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Object(4),
                TextToken::Scalar(Scalar1252::new(b"bar")),
                TextToken::Scalar(Scalar1252::new(b"qux")),
                TextToken::End(1),
            ]
        );
    }

    #[test]
    fn test_object_multi_field_event() {
        let data = b"foo={bar=1 qux=28}";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Object(6),
                TextToken::Scalar(Scalar1252::new(b"bar")),
                TextToken::Scalar(Scalar1252::new(b"1")),
                TextToken::Scalar(Scalar1252::new(b"qux")),
                TextToken::Scalar(Scalar1252::new(b"28")),
                TextToken::End(1),
            ]
        );
    }

    #[test]
    fn test_text_parser_tape() {
        let mut tape = TextTape::new();

        let data = b"foo={bar=1 qux=28}";

        TextParser::windows1252_parser()
            .parse_slice_into_tape(data, &mut tape)
            .unwrap();

        assert_eq!(
            tape.tokens(),
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Object(6),
                TextToken::Scalar(Scalar1252::new(b"bar")),
                TextToken::Scalar(Scalar1252::new(b"1")),
                TextToken::Scalar(Scalar1252::new(b"qux")),
                TextToken::Scalar(Scalar1252::new(b"28")),
                TextToken::End(1),
            ]
        );

        let data2 = b"foo2={bar2=3 qux2=29}";
        TextParser::windows1252_parser()
            .parse_slice_into_tape(data2, &mut tape)
            .unwrap();

        assert_eq!(
            tape.tokens(),
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo2")),
                TextToken::Object(6),
                TextToken::Scalar(Scalar1252::new(b"bar2")),
                TextToken::Scalar(Scalar1252::new(b"3")),
                TextToken::Scalar(Scalar1252::new(b"qux2")),
                TextToken::Scalar(Scalar1252::new(b"29")),
                TextToken::End(1),
            ]
        );
    }

    #[test]
    fn test_array_event() {
        let data = b"versions={\r\n\t\"1.28.3.0\"\r\n}";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"versions")),
                TextToken::Array(3),
                TextToken::Scalar(Scalar1252::new(b"1.28.3.0")),
                TextToken::End(1),
            ]
        );
    }

    #[test]
    fn test_array_multievent() {
        let data = b"versions={\r\n\t\"1.28.3.0\"\r\n foo}";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"versions")),
                TextToken::Array(4),
                TextToken::Scalar(Scalar1252::new(b"1.28.3.0")),
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::End(1),
            ]
        );
    }

    #[test]
    fn test_no_equal_object_event() {
        let data = b"foo{bar=qux}";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Object(4),
                TextToken::Scalar(Scalar1252::new(b"bar")),
                TextToken::Scalar(Scalar1252::new(b"qux")),
                TextToken::End(1),
            ]
        );
    }

    #[test]
    fn test_empty_array() {
        let data = b"discovered_by={}";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"discovered_by")),
                TextToken::Array(2),
                TextToken::End(1),
            ]
        );
    }

    #[test]
    fn test_array_of_objects() {
        let data = b"stats={{id=0 type=general} {id=1 type=admiral}}";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"stats")),
                TextToken::Array(14),
                TextToken::Object(7),
                TextToken::Scalar(Scalar1252::new(b"id")),
                TextToken::Scalar(Scalar1252::new(b"0")),
                TextToken::Scalar(Scalar1252::new(b"type")),
                TextToken::Scalar(Scalar1252::new(b"general")),
                TextToken::End(2),
                TextToken::Object(13),
                TextToken::Scalar(Scalar1252::new(b"id")),
                TextToken::Scalar(Scalar1252::new(b"1")),
                TextToken::Scalar(Scalar1252::new(b"type")),
                TextToken::Scalar(Scalar1252::new(b"admiral")),
                TextToken::End(8),
                TextToken::End(1),
            ]
        );
    }

    #[test]
    fn test_empty_objects2() {
        let data = b"foo={bar=val {}} me=you";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Object(4),
                TextToken::Scalar(Scalar1252::new(b"bar")),
                TextToken::Scalar(Scalar1252::new(b"val")),
                TextToken::End(1),
                TextToken::Scalar(Scalar1252::new(b"me")),
                TextToken::Scalar(Scalar1252::new(b"you")),
            ]
        );
    }

    #[test]
    fn test_spanning_objects() {
        let data = b"army={name=abc} army={name=def}";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"army")),
                TextToken::Object(4),
                TextToken::Scalar(Scalar1252::new(b"name")),
                TextToken::Scalar(Scalar1252::new(b"abc")),
                TextToken::End(1),
                TextToken::Scalar(Scalar1252::new(b"army")),
                TextToken::Object(9),
                TextToken::Scalar(Scalar1252::new(b"name")),
                TextToken::Scalar(Scalar1252::new(b"def")),
                TextToken::End(6),
            ]
        );
    }

    #[test]
    fn test_mixed_object_array() {
        // This is something that probably won't have a deserialized test
        // as ... how should one interpret it?
        let data = br#"brittany_area = { #5
            color = { 118  99  151 }
            169 170 171 172 4384
        }"#;

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"brittany_area")),
                TextToken::Object(13),
                TextToken::Scalar(Scalar1252::new(b"color")),
                TextToken::Array(7),
                TextToken::Scalar(Scalar1252::new(b"118")),
                TextToken::Scalar(Scalar1252::new(b"99")),
                TextToken::Scalar(Scalar1252::new(b"151")),
                TextToken::End(3),
                TextToken::Scalar(Scalar1252::new(b"169")),
                TextToken::Scalar(Scalar1252::new(b"170")),
                TextToken::Scalar(Scalar1252::new(b"171")),
                TextToken::Scalar(Scalar1252::new(b"172")),
                TextToken::Scalar(Scalar1252::new(b"4384")),
                TextToken::End(1),
            ]
        );
    }

    #[test]
    fn test_regression() {
        let data = [0, 32, 34, 0];
        assert!(parse(&data[..]).is_err());
    }

    #[test]
    fn test_regression2() {
        let data = [0, 4, 33, 0];
        let _ = parse(&data[..]);
    }

    #[test]
    fn test_too_heavily_nested() {
        let mut data = Vec::new();
        data.extend_from_slice(b"foo=");
        for _ in 0..100000 {
            data.extend_from_slice(b"{");
        }
        assert!(parse(&data[..]).is_err());
    }

    #[test]
    fn test_no_ws_comment() {
        let data = b"foo=abc#def\nbar=qux";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Scalar(Scalar1252::new(b"abc")),
                TextToken::Scalar(Scalar1252::new(b"bar")),
                TextToken::Scalar(Scalar1252::new(b"qux")),
            ]
        );
    }

    #[test]
    fn test_period_in_identifiers() {
        let data = b"flavor_tur.8=yes";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"flavor_tur.8")),
                TextToken::Scalar(Scalar1252::new(b"yes")),
            ]
        );
    }

    #[test]
    fn test_dashed_identifiers() {
        // From stellaris saves
        let data = b"dashed-identifier=yes";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"dashed-identifier")),
                TextToken::Scalar(Scalar1252::new(b"yes")),
            ]
        );
    }

    #[test]
    fn test_colon_values() {
        let data = b"province_id = event_target:agenda_province";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"province_id")),
                TextToken::Scalar(Scalar1252::new(b"event_target:agenda_province")),
            ]
        );
    }

    #[test]
    fn test_variables() {
        let data = b"@planet_standard_scale = 11";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"@planet_standard_scale")),
                TextToken::Scalar(Scalar1252::new(b"11")),
            ]
        );
    }

    #[test]
    fn test_equal_identifier() {
        let data = br#"=="bar""#;

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"=")),
                TextToken::Scalar(Scalar1252::new(b"bar")),
            ]
        );
    }

    #[test]
    fn test_many_line_comment() {
        let mut data = Vec::new();
        data.extend_from_slice(b"foo=1.000\n");
        for _ in 0..100000 {
            data.extend_from_slice(b"# this is a comment\n");
        }
        data.extend_from_slice(b"foo=2.000\n");

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Scalar(Scalar1252::new(b"1.000")),
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Scalar(Scalar1252::new(b"2.000")),
            ]
        );
    }

    #[test]
    fn test_terminating_comment() {
        let data = b"# boo\r\n# baa\r\nfoo=a\r\n# bee";
        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Scalar(Scalar1252::new(b"a")),
            ]
        );
    }

    #[test]
    fn test_rgb_trick() {
        let data = b"name = rgb ";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"name")),
                TextToken::Scalar(Scalar1252::new(b"rgb")),
            ]
        );
    }

    #[test]
    fn test_rgb_trick2() {
        let data = b"name = rgb type = 4713";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"name")),
                TextToken::Scalar(Scalar1252::new(b"rgb")),
                TextToken::Scalar(Scalar1252::new(b"type")),
                TextToken::Scalar(Scalar1252::new(b"4713")),
            ]
        );
    }

    #[test]
    fn test_rgb_trick3() {
        let data = b"name = rgbeffect";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"name")),
                TextToken::Scalar(Scalar1252::new(b"rgbeffect")),
            ]
        );
    }

    #[test]
    fn test_rgb() {
        let data = b"color = rgb { 100 200 150 } ";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"color")),
                TextToken::Rgb(Box::new(Rgb {
                    r: 100,
                    g: 200,
                    b: 150,
                })),
            ]
        );
    }

    #[test]
    fn test_heterogenous_list() {
        let data = b"levels={ 10 0=2 1=2 } foo={bar=qux}";
        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"levels")),
                TextToken::Array(9),
                TextToken::Scalar(Scalar1252::new(b"10")),
                TextToken::Object(8),
                TextToken::Scalar(Scalar1252::new(b"0")),
                TextToken::Scalar(Scalar1252::new(b"2")),
                TextToken::Scalar(Scalar1252::new(b"1")),
                TextToken::Scalar(Scalar1252::new(b"2")),
                TextToken::End(3),
                TextToken::End(1),
                TextToken::Scalar(Scalar1252::new(b"foo")),
                TextToken::Object(14),
                TextToken::Scalar(Scalar1252::new(b"bar")),
                TextToken::Scalar(Scalar1252::new(b"qux")),
                TextToken::End(11),
            ]
        );
    }

    #[test]
    fn test_hidden_object() {
        let data = b"16778374={ levels={ 10 0=2 1=2 } }";

        assert_eq!(
            parse(&data[..]).unwrap().token_tape,
            vec![
                TextToken::Scalar(Scalar1252::new(b"16778374")),
                TextToken::Object(12),
                TextToken::Scalar(Scalar1252::new(b"levels")),
                TextToken::Array(11),
                TextToken::Scalar(Scalar1252::new(b"10")),
                TextToken::Object(10),
                TextToken::Scalar(Scalar1252::new(b"0")),
                TextToken::Scalar(Scalar1252::new(b"2")),
                TextToken::Scalar(Scalar1252::new(b"1")),
                TextToken::Scalar(Scalar1252::new(b"2")),
                TextToken::End(5),
                TextToken::End(3),
                TextToken::End(1),
            ]
        );
    }

    #[test]
    fn test_initial_end_does_not_panic() {
        let res = parse(&b"}"[..]);
        assert!(res.is_ok() || res.is_err());
    }

    #[test]
    fn test_utf8_parser() {
        let data = r#"meta_title_name="Chiefdom of Jåhkåmåhkke""#;
        let tape = TextParser::from_utf8(data.as_bytes()).unwrap();
        assert_eq!(
            tape.tokens(),
            vec![
                TextToken::Scalar(ScalarUtf8::new("meta_title_name".as_bytes())),
                TextToken::Scalar(ScalarUtf8::new("Chiefdom of Jåhkåmåhkke".as_bytes())),
            ]
        );
    }
}
