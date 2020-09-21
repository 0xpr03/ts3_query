//! Module with helpers for raw-calls
use std::{collections::HashMap, str::FromStr};

use snafu::ResultExt;

/// Parse response as hashmap
///
/// unescape: if true unescapes values, can be turned off to boost performance on known response types (like numbers)
///
/// ```rust
/// use ts3_query::*;
/// use std::collections::HashMap;
///
/// let input: Vec<String> = vec!["clid=28631 cid=9391 foo","client_type=1",""]
///     .into_iter().map(ToOwned::to_owned).collect();
/// let expected: HashMap<String, Option<String>> = vec![
///     ("clid",Some("28631")),
///     ("cid",Some("9391")),
///     ("foo",None),
///     ("client_type",Some("1"))]
///     .into_iter().map(|(x,y)|(x.to_owned(),y.map(|y|y.to_owned()))).collect();
/// assert_eq!(expected,raw::parse_hashmap(input,false));
/// ```
pub fn parse_hashmap(input: Vec<String>, unescape: bool) -> HashMap<String, Option<String>> {
    let mut map: HashMap<String, Option<String>> = HashMap::new();
    input.into_iter().for_each(|s| {
        parse_single_line_hashmap(&s, &mut map, unescape);
    });
    map
}

/// Parse a single hashmap, not able to handle lists, see parse_multi_hashmap.
fn parse_single_line_hashmap(
    line: &str,
    map: &mut HashMap<String, Option<String>>,
    unescape: bool,
) {
    line.split_whitespace().for_each(|e| {
        let mut entries = e.split('=');
        if let (Some(k), Some(v)) = (entries.next(), entries.next()) {
            let v = if unescape {
                unescape_val(v)
            } else {
                v.to_string()
            };
            map.insert(k.to_string(), Some(v));
        } else if !e.is_empty() {
            map.insert(e.to_string(), None);
        }
    });
}

/// Parse multi-hashmap response. Each hashmap is divided by a `|`.
///
/// Example input: for clientlist, 3 clients
/// ```text
/// clid=1776 cid=9391 client_database_id=18106 client_nickname=FOOBAR\\s\\p\\sNora\\s\\p\\sLaptop
/// client_type=1|clid=1775 cid=9402 client_database_id=136830 ///client_nickname=ASDF\\/FGHJ\\/Dewran client_type=0|
/// clid=1 cid=24426 client_database_id=18106 client_nickname=bot client_type=1
/// ```
pub fn parse_multi_hashmap(
    input: Vec<String>,
    unescape: bool,
) -> Vec<HashMap<String, Option<String>>> {
    let v: Vec<HashMap<String, Option<String>>> = input
        .into_iter()
        .map(|l| {
            l.split('|')
                .map(|s| {
                    let mut map = HashMap::new();
                    parse_single_line_hashmap(s, &mut map, unescape);
                    map
                })
                .collect::<Vec<HashMap<String, Option<String>>>>()
        })
        .flatten()
        .collect();
    v
}

/// Escape string for query commands send via raw function
pub fn escape_arg<T: AsRef<str>>(input: T) -> String {
    let res: Vec<u8> = Escape::new(input.as_ref().bytes()).collect();
    String::from_utf8(res).unwrap()
}

/// Unescape server response
pub fn unescape_val<T: AsRef<str>>(it: T) -> String {
    let mut res: Vec<u8> = Vec::new();
    let mut escaped = false;
    for n in it.as_ref().as_bytes().iter() {
        if !escaped && *n == b'\\' {
            escaped = true;
        } else if escaped {
            let ch = match n {
                b's' => b' ',
                b'p' => b'|',
                b'a' => 7,
                b'b' => 8,
                b'f' => 12,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'v' => 11,
                _ => *n, // matches \\ \/ also
            };
            res.push(ch);
            escaped = false;
        } else {
            res.push(*n);
        }
    }
    unsafe {
        // we know this is utf8 as we only added utf8 strings using fmt
        String::from_utf8_unchecked(res)
    }
}

const LONGEST_ESCAPE: usize = 2;

/// Escape function for commands
///
/// Can be used like Escape::new(String)
#[must_use = "iterator adaptors are lazy and do nothing unless consumed"]
struct Escape<I: Iterator<Item = u8>> {
    inner: I,
    buffer: u8,
}

impl<I: Iterator<Item = u8>> Escape<I> {
    /// Create an iterator adaptor which will escape all the bytes of internal iterator.
    pub fn new(i: I) -> Escape<I> {
        Escape {
            inner: i,
            buffer: 0,
        }
    }
}

impl<I: Iterator<Item = u8>> Iterator for Escape<I> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        if self.buffer != 0 {
            let ret = Some(self.buffer as u8);
            self.buffer = 0;
            ret
        } else if let Some(ch) = self.inner.next() {
            match ch {
                // reverse hex representation
                // as we take them in that order
                b'\\' | b'/' => {
                    self.buffer = ch;
                    Some(b'\\')
                }
                b' ' => {
                    self.buffer = b's';
                    Some(b'\\')
                }
                b'|' => {
                    self.buffer = b'p';
                    Some(b'\\')
                }
                7 => {
                    self.buffer = b'a';
                    Some(b'\\')
                }
                8 => {
                    self.buffer = b'b';
                    Some(b'\\')
                }
                12 => {
                    self.buffer = b'f';
                    Some(b'\\')
                }
                b'\n' => {
                    self.buffer = b'n';
                    Some(b'\\')
                }
                b'\r' => {
                    self.buffer = b'r';
                    Some(b'\\')
                }
                b'\t' => {
                    self.buffer = b't';
                    Some(b'\\')
                }
                11 => {
                    self.buffer = b'v';
                    Some(b'\\')
                }
                _ => Some(ch),
            }
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (l, u) = self.inner.size_hint();
        (
            l,
            if let Some(u_) = u {
                u_.checked_mul(LONGEST_ESCAPE)
            } else {
                None
            },
        )
    }
}

/// Helper function to read int value list from line-hashmap, (re)moves value.
///
/// ```rust
/// use ts3_query::*;
/// use std::collections::HashMap;
///
/// let mut v: HashMap<String, Option<String>> =
///     vec![("abc".to_string(), Some("123,345,123".to_string())),
///     ("def".to_string(), None)]
///     .into_iter().collect();
/// let v: Vec<i32> = raw::int_list_val_parser(&mut v, "abc").unwrap();
/// assert_eq!(vec![123,345,123],v);
/// ```
pub fn int_list_val_parser<T>(
    data: &mut HashMap<String, Option<String>>,
    key: &'static str,
) -> crate::Result<Vec<T>>
where
    T: FromStr<Err = std::num::ParseIntError>,
{
    let v = string_val_parser(data, key)?;
    let values: Vec<T> = v
        .split(",")
        .map(|v| {
            v.parse::<T>()
                .with_context(|| crate::InvalidIntResponse { data: v })
        })
        .collect::<crate::Result<Vec<T>>>()?;

    Ok(values)
}

/// Helper function to retrieve bool value from line-hashmap, (re)moves value.
///
/// ```rust
/// use ts3_query::*;
/// use std::collections::HashMap;
///
/// let mut v: HashMap<String, Option<String>> =
///     vec![("abc".to_string(), Some("1".to_string())),
///     ("def".to_string(), Some("0".to_string()))]
///     .into_iter().collect();
/// assert_eq!(true,raw::bool_val_parser(&mut v, "abc").unwrap());
/// assert_eq!(false,raw::bool_val_parser(&mut v, "def").unwrap());
/// assert!(raw::bool_val_parser(&mut v, "foobar").is_err());
/// ```
pub fn bool_val_parser(
    data: &mut HashMap<String, Option<String>>,
    key: &'static str,
) -> crate::Result<bool> {
    let val: i32 = int_val_parser(data, key)?;
    Ok(val > 0)
}

/// Helper function to retrieve optional string value from line-hashmap, (re)moves value.
///
/// ```rust
/// use ts3_query::*;
/// use std::collections::HashMap;
///
/// let mut v: HashMap<String, Option<String>> =
///     vec![("abc".to_string(), Some("asd".to_string())),
///     ("def".to_string(), None)]
///     .into_iter().collect();
/// assert_eq!(Some("asd".to_string()),raw::string_val_parser_opt(&mut v, "abc").unwrap());
/// assert_eq!(None,raw::string_val_parser_opt(&mut v, "def").unwrap());
/// ```
pub fn string_val_parser_opt(
    data: &mut HashMap<String, Option<String>>,
    key: &'static str,
) -> crate::Result<Option<String>> {
    Ok(data
        .remove(key)
        .ok_or_else(|| crate::NoEntryResponse { key }.build())?
        .map(unescape_val))
}

/// Helper function to retrieve and parse value from line-hashmap, (re)moves value.
///
/// ```rust
/// use ts3_query::*;
/// use std::collections::HashMap;
///
/// let mut v: HashMap<String, Option<String>> =
///     vec![("abc".to_string(), Some("123".to_string())),
///     ("def".to_string(), None)]
///     .into_iter().collect();
/// assert_eq!(Some(123),raw::int_val_parser_opt::<i32>(&mut v, "abc").unwrap());
/// assert_eq!(None,raw::int_val_parser_opt::<i32>(&mut v, "def").unwrap());
/// ```
pub fn int_val_parser_opt<T>(
    data: &mut HashMap<String, Option<String>>,
    key: &'static str,
) -> crate::Result<Option<T>>
where
    T: FromStr<Err = std::num::ParseIntError>,
{
    let v = data
        .remove(key)
        .ok_or_else(|| crate::NoEntryResponse { key }.build())?;

    if let Some(v) = v {
        return Ok(Some(
            v.parse()
                .with_context(|| crate::InvalidIntResponse { data: v })?,
        ));
    } else {
        return Ok(None);
    }
}

/// Helper function to retrieve and parse value from line-hashmap, (re)moves value.
///
/// ```rust
/// use ts3_query::*;
/// use std::collections::HashMap;
///
/// let mut v: HashMap<String, Option<String>> =
///     vec![("abc".to_string(), Some("123".to_string())),
///     ("def".to_string(), None)]
///     .into_iter().collect();
/// assert_eq!(123,raw::int_val_parser::<i32>(&mut v, "abc").unwrap());
/// assert!(raw::int_val_parser::<i32>(&mut v, "def").is_err());
/// ```
pub fn int_val_parser<T>(
    data: &mut HashMap<String, Option<String>>,
    key: &'static str,
) -> crate::Result<T>
where
    T: FromStr<Err = std::num::ParseIntError>,
{
    let v = data
        .remove(key)
        .ok_or_else(|| crate::NoEntryResponse { key }.build())?
        .ok_or_else(|| crate::NoValueResponse { key }.build())?;
    Ok(v.parse()
        .with_context(|| crate::InvalidIntResponse { data: v })?)
}

/// Helper function to retrieve string value from line-hashmap, (re)moves value.
///
/// ```rust
/// use ts3_query::*;
/// use std::collections::HashMap;
///
/// let mut v: HashMap<String, Option<String>> =
///     vec![("abc".to_string(), Some("asd".to_string())),
///     ("def".to_string(), None)]
///     .into_iter().collect();
/// assert_eq!("asd".to_string(),raw::string_val_parser(&mut v, "abc").unwrap());
/// assert!(raw::string_val_parser(&mut v, "def").is_err());
/// ```
pub fn string_val_parser(
    data: &mut HashMap<String, Option<String>>,
    key: &'static str,
) -> crate::Result<String> {
    Ok(string_val_parser_opt(data, key)?.ok_or_else(|| crate::NoValueResponse { key }.build())?)
}

#[cfg(test)]
mod test {
    use super::*;
    /// Verify all escape sequences are valid utf-8 the easy way.
    /// Otherwise the conversion in read_response would be invalid as we're not un-escaping before converting it into a string.
    ///
    /// This also enforces the invariant of our unsafe utf8 conversion on unescaping.
    #[test]
    pub fn test_escaped_input() {
        let v: Vec<u8> = vec![b'\\', b'/', 7, 8, 12, 11, b'\t', b'\r', b'\n'];

        assert!(true, String::from_utf8(v).is_ok());
    }

    #[test]
    pub fn verify_single_map() {
        let v = "clid=1776 client_database_id=18106 client_nickname=FOOBAR\\s\\p\\sNora\\s\\p\\sLaptop client_type=1";
        let mut map = HashMap::new();
        parse_single_line_hashmap(v, &mut map, false);
        assert_eq!(
            Some("1776"),
            map.get("clid")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        assert_eq!(
            Some("18106"),
            map.get("client_database_id")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        assert_eq!(
            Some("FOOBAR\\s\\p\\sNora\\s\\p\\sLaptop"),
            map.get("client_nickname")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        assert_eq!(
            Some("1"),
            map.get("client_type")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        // verify public function does the same
        assert_eq!(map, parse_hashmap(vec![v.to_string()], false));

        let mut map = HashMap::new();
        parse_single_line_hashmap(v, &mut map, true);
        assert_eq!(
            Some("1776"),
            map.get("clid")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        assert_eq!(
            Some("18106"),
            map.get("client_database_id")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        assert_eq!(
            Some(r#"FOOBAR | Nora | Laptop"#),
            map.get("client_nickname")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        assert_eq!(
            Some("1"),
            map.get("client_type")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        // verify public function does the same
        assert_eq!(map, parse_hashmap(vec![v.to_string()], true));
    }

    #[test]
    pub fn verify_single_map_optional() {
        let v = "client_type=123 client_away=456 client_away_message client_flag_talking=789";
        let mut map = HashMap::new();
        parse_single_line_hashmap(v, &mut map, false);

        let mut expected = HashMap::new();
        expected.insert("client_type".to_string(), Some("123".to_string()));
        expected.insert("client_away".to_string(), Some("456".to_string()));
        expected.insert("client_away_message".to_string(), None);
        expected.insert("client_flag_talking".to_string(), Some("789".to_string()));

        assert_eq!(map, expected);
    }

    #[test]
    pub fn verify_multi_map() {
        let v = "clid=1776 client_database_id=18106|client_nickname=FOOBAR\\s\\p\\sNora\\s\\p\\sLaptop client_type=1";
        let result = parse_multi_hashmap(vec![v.to_string()], true);
        let first = &result[0];
        assert_eq!(
            Some("1776"),
            first
                .get("clid")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        assert_eq!(
            Some("18106"),
            first
                .get("client_database_id")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        let second = &result[1];
        assert_eq!(
            Some(r#"FOOBAR | Nora | Laptop"#),
            second
                .get("client_nickname")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
        assert_eq!(
            Some("1"),
            second
                .get("client_type")
                .map(|v| v.as_ref().map(|v| v.as_str()))
                .flatten()
        );
    }
}
