//! Module with helpers for raw-calls
use std::collections::HashMap;

/// Parse response as hashmap
///
/// unescape: if true unescapes values, can be turned off to boost performance on known response types (numbers..)
pub fn parse_hashmap(input: Vec<String>, unescape: bool) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    input.into_iter().for_each(|s| {
        s.split_whitespace().for_each(|e| {
            let mut entries = e.split('=');
            if let (Some(k), Some(v)) = (entries.next(), entries.next()) {
                let v = if unescape {
                    unescape_val(v)
                } else {
                    v.to_string()
                };
                map.insert(k.to_string(), v);
            }
        });
    });
    map
}

/// Escape string for query commands send via raw function
pub fn escape_arg(input: &str) -> String {
    let res: Vec<u8> = Escape::new(input.bytes()).collect();
    String::from_utf8(res).unwrap()
}

/// Unescape server response
pub fn unescape_val(it: &str) -> String {
    let mut res: Vec<u8> = Vec::new();
    let mut escaped = false;
    for n in it.as_bytes().iter() {
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

#[cfg(test)]
mod test {
    /// Verify all escape sequences are valid utf-8 the easy way.
    /// Otherwise the conversion in read_response would be invalid as we're not un-escaping before converting it into a string.
    /// 
    /// This also enforces the invariant of our unsafe utf8 conversion on unescaping.
    #[test]
    pub fn test_escaped_input() {
        let v: Vec<u8> = vec![b'\\', b'/', 7, 8, 12, 11, b'\t', b'\r', b'\n'];

        assert!(true, String::from_utf8(v).is_ok());
    }
}