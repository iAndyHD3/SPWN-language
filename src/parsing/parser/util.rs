use std::str::Chars;

use base64::Engine;
use lasso::Spur;
use unindent::unindent;

use super::{ParseResult, Parser};
use crate::gd::ids::IDClass;
use crate::lexing::tokens::Token;
use crate::list_helper;
use crate::parsing::ast::{
    DictItem, Expression, Import, ImportSettings, ImportType, StringContent, StringType, Vis,
};
use crate::parsing::attributes::{Attributes, IsValidOn, ParseAttribute};
use crate::parsing::error::SyntaxError;
use crate::sources::{CodeSpan, Spannable, Spanned};

impl Parser<'_> {
    pub fn parse_int(&self, s: &str, base: u32) -> i64 {
        i64::from_str_radix(&s.replace('_', ""), base).unwrap()
    }

    pub fn parse_golden_float(&self, s: &str) -> f64 {
        let mut n = 0_f64;
        for (i, d) in s.bytes().enumerate() {
            let pow = s.len() - 1 - i;
            const PHI: f64 = 1.6180339887498948482045868343656381177203091798057;
            n += PHI.powf(pow as f64) * if d == b'0' { 0.0 } else { 1.0 };
        }
        n
    }

    pub fn parse_id(&self, s: &str, class: IDClass) -> Expression {
        let value = s[0..(s.len() - 1)].parse::<u16>().ok();

        Expression::Id(class, value)
    }

    pub fn parse_string(&mut self, start_tok: Token) -> ParseResult<StringContent> {
        let start_slice = self.slice();

        Ok(match start_tok {
            Token::String => {
                let s = &start_slice[1..(start_slice.len() - 1)];
                let s = self.parse_plain_string(s)?;
                StringContent {
                    s: StringType::Normal(self.intern_string(s)),
                    bytes: false,
                    base64: false,
                    unindent: false,
                }
            },
            Token::RawString => {
                let s = &start_slice[1..];
                let b = s.as_bytes();

                let mut i = 0;
                loop {
                    if matches!(b[i], b'"' | b'\'') {
                        break;
                    }
                    i += 1;
                }

                StringContent {
                    s: StringType::Normal(self.intern_string(&s[(i + 1)..(s.len() - 1 - i)])),

                    bytes: false,
                    base64: false,
                    unindent: false,
                }
            },
            Token::StringFlags => {
                let mut is_bytes = false;
                let mut is_unindent = false;
                let mut is_base64 = false;

                for i in start_slice.bytes() {
                    let flag = match i {
                        b'b' => &mut is_bytes,
                        b'B' => &mut is_base64,
                        b'u' => &mut is_unindent,
                        f => {
                            return Err(SyntaxError::UnexpectedFlag {
                                flag: (f as char).to_string(),
                                area: self.make_area(self.span()),
                            })
                        },
                    };
                    *flag = true;
                }

                let t = self.next()?;
                let mut content = self.parse_string(t)?;

                content.bytes = is_bytes;
                content.base64 = is_base64;
                content.unindent = is_unindent;

                content
            },
            _ => unreachable!(),
        })
    }

    pub fn parse_plain_string(&self, s: &str) -> ParseResult<String> {
        self.parse_escapes(&mut s[1..(s.len() - 1)].chars())
    }

    pub fn parse_compile_time_string(&mut self) -> ParseResult<String> {
        let s = self
            .parse_string(Token::String)?
            .get_compile_time(&self.interner)
            .ok_or(SyntaxError::InvalidDictStringKey {
                area: self.make_area(self.span()),
            })?;

        Ok(s)
    }

    pub fn parse_escapes(&self, chars: &mut Chars) -> ParseResult<String> {
        let mut out = String::new();

        loop {
            match chars.next() {
                Some('\\') => out.push(match chars.next() {
                    Some('n') => '\n',
                    Some('r') => '\r',
                    Some('t') => '\t',
                    Some('"') => '"',
                    Some('\'') => '\'',
                    Some('\\') => '\\',
                    Some('u') => self.parse_unicode(chars)?,
                    Some(c) => {
                        return Err(SyntaxError::InvalidEscape {
                            character: c,
                            area: self.make_area(self.span()),
                        })
                    },
                    None => {
                        return Err(SyntaxError::InvalidEscape {
                            character: ' ',
                            area: self.make_area(self.span()),
                        })
                    },
                }),
                Some(c) => out.push(c),
                None => break,
            }
        }

        Ok(out)
    }

    pub fn parse_unicode(&self, chars: &mut Chars) -> ParseResult<char> {
        let next = chars.next().unwrap_or(' ');

        if !matches!(next, '{') {
            return Err(SyntaxError::UnxpectedCharacter {
                expected: Token::LBracket,
                found: next.to_string(),
                area: self.make_area(self.span()),
            });
        }

        // `take_while` will always consume the matched chars +1 (in order to check whether it matches)
        // this means `unwrap_or` would always use the default, so instead clone it to not affect
        // the actual chars iterator
        let hex = chars
            .clone()
            .take_while(|c| matches!(*c, '0'..='9' | 'a'..='f' | 'A'..='F'))
            .collect::<String>();

        let mut schars = chars.skip(hex.len());

        let next = schars.next();

        match next {
            Some('}') => (),
            Some(t) => {
                return Err(SyntaxError::UnxpectedCharacter {
                    expected: Token::RBracket,
                    found: t.to_string(),
                    area: self.make_area(self.span()),
                })
            },
            None => {
                return Err(SyntaxError::UnxpectedCharacter {
                    expected: Token::RBracket,
                    found: "end of string".into(),
                    area: self.make_area(self.span()),
                })
            },
        }

        Ok(char::from_u32(u32::from_str_radix(&hex, 16).map_err(|_| {
            SyntaxError::InvalidUnicode {
                sequence: hex,
                area: self.make_area(self.span()),
            }
        })?)
        .unwrap_or('\u{FFFD}'))
    }

    pub fn parse_dictlike(&mut self, allow_vis: bool) -> ParseResult<Vec<Vis<DictItem>>> {
        let mut items = vec![];

        list_helper!(self, RBracket {
            let attrs = if self.skip_tok(Token::Hashtag)? {

                self.parse_attributes::<Attributes>()?
            } else {
                vec![]
            };

            let start = self.peek_span()?;

            let vis = if allow_vis && self.next_is(Token::Private)? {
                self.next()?;
                Vis::Private
            } else {
                Vis::Public
            };

            let key = match self.next()? {
                Token::Int => self.intern_string(self.parse_int(self.slice(), 10).to_string()),
                Token::HexInt => self.intern_string(self.parse_int(&self.slice()[2..], 16).to_string()),
                Token::OctalInt => self.intern_string(self.parse_int(&self.slice()[2..], 8).to_string()),
                Token::BinaryInt => self.intern_string(self.parse_int(&self.slice()[2..], 2).to_string()),
                Token::SeximalInt => self.intern_string(self.parse_int(&self.slice()[2..], 6).to_string()),
                Token::DozenalInt => self.intern_string(self.parse_int(&self.slice()[2..], 12).to_string()),
                Token::String => {
                    let s = self.parse_compile_time_string()?;
                    self.intern_string(s)
                },
                Token::Ident => self.intern_string(self.slice()),
                other => {
                    return Err(SyntaxError::UnexpectedToken {
                        expected: "key".into(),
                        found: other,
                        area: self.make_area(self.span()),
                    })
                }
            };

            let key_span = self.span();

            let elem = if self.next_is(Token::Colon)? {
                self.next()?;
                Some(self.parse_expr(true)?)
            } else {
                None
            };

            // this is so backwards if only u could use enum variants as types. . . .
            let mut item = DictItem { name: key.spanned(key_span), attributes: vec![], value: elem }.spanned(start.extend(self.span()));

            attrs.is_valid_on(&item, &self.src)?;

            item.attributes = attrs;

            items.push(vis(item.value));
        });

        Ok(items)
    }

    pub fn parse_attributes<T: ParseAttribute>(&mut self) -> ParseResult<Vec<Spanned<T>>> {
        let mut attrs = vec![];
        self.expect_tok(Token::LSqBracket)?;

        list_helper!(self, RSqBracket {
            let start = self.peek_span()?;
            attrs.push(T::parse(self)?.spanned(start.extend(self.span())))
        });

        Ok(attrs)
    }

    pub fn parse_import(&mut self) -> ParseResult<Import> {
        Ok(match self.peek()? {
            Token::String => {
                self.next()?;

                Import {
                    path: self.parse_plain_string(self.slice())?.into(),
                    settings: ImportSettings {
                        typ: ImportType::File,
                        is_absolute: false,
                        allow_builtin_impl: false,
                    },
                }
            },
            Token::Ident => {
                self.next()?;
                Import {
                    path: self.slice().into(),
                    settings: ImportSettings {
                        typ: ImportType::Library,
                        is_absolute: false,
                        allow_builtin_impl: false,
                    },
                }
            },
            other => {
                return Err(SyntaxError::UnexpectedToken {
                    expected: "string literal or identifier".into(),
                    found: other,
                    area: self.make_area(self.peek_span()?),
                })
            },
        })
    }
}
