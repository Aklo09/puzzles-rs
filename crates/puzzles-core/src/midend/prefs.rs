//! Preferences files (C's `midend_save_prefs`, `midend_load_prefs`): one
//! `keyword=value` line per preference. A value's newlines are written
//! as a newline and a space. Lines starting with `#` and blank lines are
//! skipped, and unknown keywords are ignored (they may come from a later
//! version).

use std::io::{self, Read, Write};

use super::Midend;
use super::savefile::{DeserialiseError, c_str, read_byte};
use crate::config::{ConfigItem, ConfigValue};
use crate::game::Game;

impl<G: Game> Midend<G> {
    /// Writes the preferences file (C's `midend_save_prefs`): the current
    /// UI's preferences, or the saved ones before the first game.
    ///
    /// # Errors
    /// Writing errors.
    pub fn save_prefs(&self, w: &mut impl Write) -> io::Result<()> {
        write_prefs(&self.get_prefs(None), w)
    }

    /// Reads a preferences file (C's `midend_load_prefs`). They apply to
    /// UIs created from now on (new games, loaded files), not to the
    /// current one, as in C. Nothing changes if the file is rejected.
    ///
    /// # Errors
    /// C's message for a malformed file; reading errors.
    pub fn load_prefs(&mut self, r: &mut impl Read) -> Result<(), DeserialiseError> {
        let mut items = self.get_prefs(None);
        read_prefs(&mut items, r)?;
        self.set_prefs(None, &items);
        Ok(())
    }
}

/// C's `midend_serialise_prefs`.
fn write_prefs(items: &[ConfigItem], w: &mut impl Write) -> io::Result<()> {
    for item in items {
        w.write_all(item.keyword.unwrap_or("").as_bytes())?;
        w.write_all(b"=")?;
        match &item.value {
            ConfigValue::Bool(b) => w.write_all(if *b { b"true" } else { b"false" })?,
            ConfigValue::String(s) => {
                for &c in s.as_bytes() {
                    w.write_all(&[c])?;
                    if c == b'\n' {
                        w.write_all(b" ")?;
                    }
                }
            }
            ConfigValue::Choices {
                names,
                keywords,
                selected,
            } => {
                let keywords = keywords.unwrap_or(names);
                w.write_all(keywords.get(*selected).copied().unwrap_or("").as_bytes())?;
            }
        }
        w.write_all(b"\n")?;
    }
    Ok(())
}

/// C's `midend_deserialise_prefs`: updates `items` from the file.
fn read_prefs(items: &mut [ConfigItem], r: &mut impl Read) -> Result<(), DeserialiseError> {
    let invalid = DeserialiseError::Invalid;
    let mut ungot = None;
    let mut eof = false;
    let mut buf = Vec::new();

    while !eof {
        let mut c = match ungot.take() {
            Some(c) => c,
            // The end at the start of a line is a normal end.
            None => match read_byte(r)? {
                Some(c) => c,
                None => return Ok(()),
            },
        };

        if c == b'#' || c == b'\n' {
            while c != b'\n' {
                match read_byte(r)? {
                    Some(next) => c = next,
                    None => return Ok(()),
                }
            }
            continue;
        }

        buf.clear();
        loop {
            buf.push(c);
            c = read_byte(r)?.ok_or(invalid("Partial line at end of preferences file"))?;
            if c == b'\n' {
                return Err(invalid("Expected '=' after keyword"));
            }
            if c == b'=' {
                break;
            }
        }
        let keyword = c_str(buf.clone());
        let item = items
            .iter_mut()
            .rfind(|it| it.keyword.is_some_and(|k| k.as_bytes() == keyword));

        buf.clear();
        loop {
            match read_byte(r)? {
                // A missing newline at the very end is fine.
                None => {
                    eof = true;
                    break;
                }
                Some(b'\n') => match read_byte(r)? {
                    // A newline then a space continues the value.
                    Some(b' ') => buf.push(b'\n'),
                    Some(next) => {
                        ungot = Some(next);
                        break;
                    }
                    None => {
                        eof = true;
                        break;
                    }
                },
                Some(c) => buf.push(c),
            }
        }

        let Some(item) = item else {
            continue;
        };
        match &mut item.value {
            ConfigValue::Bool(b) => {
                *b = match c_str(buf.clone()).as_slice() {
                    b"true" => true,
                    b"false" => false,
                    _ => return Err(invalid("Value for boolean was not 'true' or 'false'")),
                };
            }
            ConfigValue::String(s) => {
                *s = String::from_utf8_lossy(&c_str(buf.clone())).into_owned();
            }
            ConfigValue::Choices {
                names,
                keywords,
                selected,
            } => {
                let keywords = keywords.unwrap_or(names);
                *selected = keywords
                    .iter()
                    .position(|k| k.as_bytes() == buf.as_slice())
                    .ok_or(invalid("Invalid value for enumeration"))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<ConfigItem> {
        vec![
            ConfigItem {
                name: "Flag",
                keyword: Some("flag"),
                value: ConfigValue::Bool(true),
            },
            ConfigItem {
                name: "Text",
                keyword: Some("text"),
                value: ConfigValue::String("a\nb".into()),
            },
            ConfigItem {
                name: "Choice",
                keyword: Some("choice"),
                value: ConfigValue::Choices {
                    names: &["One", "Two"],
                    keywords: Some(&["one", "two"]),
                    selected: 1,
                },
            },
        ]
    }

    #[test]
    fn writes_c_format_and_reads_it_back() {
        let mut out = Vec::new();
        write_prefs(&items(), &mut out).unwrap();
        assert_eq!(out, b"flag=true\ntext=a\n b\nchoice=two\n");

        let mut read = items();
        for item in &mut read {
            item.value = match &item.value {
                ConfigValue::Bool(_) => ConfigValue::Bool(false),
                ConfigValue::String(_) => ConfigValue::String(String::new()),
                ConfigValue::Choices {
                    names, keywords, ..
                } => ConfigValue::Choices {
                    names,
                    keywords: *keywords,
                    selected: 0,
                },
            };
        }
        read_prefs(&mut read, &mut out.as_slice()).unwrap();
        assert_eq!(read, items());
    }

    #[test]
    fn comments_unknown_keywords_and_missing_newline() {
        let mut read = items();
        read_prefs(&mut read, &mut &b"# comment\n\nother=x\nflag=false"[..]).unwrap();
        assert_eq!(read[0].as_bool(), Some(false));
    }

    #[test]
    fn errors_are_cs() {
        for (text, message) in [
            (&b"flag"[..], "Partial line at end of preferences file"),
            (b"flag\n", "Expected '=' after keyword"),
            (b"flag=yes\n", "Value for boolean was not 'true' or 'false'"),
            (b"choice=three\n", "Invalid value for enumeration"),
        ] {
            let err = read_prefs(&mut items(), &mut &text[..]).unwrap_err();
            assert_eq!(err.to_string(), message);
        }
    }
}
