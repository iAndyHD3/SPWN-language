mod exprs;
mod patterns;
mod stmts;
mod util;

use std::cell::RefCell;
use std::rc::Rc;

use lasso::Spur;

use super::ast::Ast;
use super::attributes::FileAttribute;
use super::error::SyntaxError;
use crate::lexing::lexer::{Lexer, LexerError};
use crate::lexing::tokens::Token;
use crate::sources::{CodeArea, CodeSpan, SpwnSource};
use crate::util::Interner;

#[derive(Clone)]
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    pub src: Rc<SpwnSource>,
    interner: Rc<RefCell<Interner>>,
}

pub type ParseResult<T> = Result<T, SyntaxError>;

impl<'a> Parser<'a> {
    pub fn new(code: &'a str, src: Rc<SpwnSource>, interner: Rc<RefCell<Interner>>) -> Self {
        let lexer = Lexer::new(code);
        Parser {
            lexer,
            src,
            interner,
        }
    }
}

#[macro_export]
macro_rules! list_helper {
    ($self:ident, $closing_tok:ident $code:block) => {
        while !$self.next_is(Token::$closing_tok)? {
            $code;
            if !$self.skip_tok(Token::Comma)? {
                break;
            }
        }
        $self.expect_tok(Token::$closing_tok)?;
    };

    ($self:ident, $first:ident, $closing_tok:ident $code:block) => {
        let mut $first = true;
        while !$self.next_is(Token::$closing_tok)? {
            $code;
            $first = false;
            if !$self.skip_tok(Token::Comma)? {
                break;
            }
        }
        $self.expect_tok(Token::$closing_tok)?;
    };
}

impl Parser<'_> {
    fn map_lexer_err(&self, e: LexerError) -> SyntaxError {
        SyntaxError::LexingError {
            err: e,
            area: self.make_area(self.span()),
        }
    }

    pub fn next(&mut self) -> ParseResult<Token> {
        let out = self
            .lexer
            .next_or_eof()
            .map_err(|e| self.map_lexer_err(e))?;

        if out == Token::Newline {
            self.next()
        } else {
            Ok(out)
        }
    }

    // pub fn next_or_newline(&mut self) -> Token {
    //     self.lexer.next_or_eof()
    // }

    pub fn span(&self) -> CodeSpan {
        self.lexer.span().into()
    }

    pub fn peek_span(&self) -> ParseResult<CodeSpan> {
        let mut peek = self.lexer.clone();
        while peek.next_or_eof().map_err(|e| self.map_lexer_err(e))? == Token::Newline {}
        Ok(peek.span().into())
    }

    // pub fn peek_span_or_newline(&self) -> CodeSpan {
    //     let mut peek = self.lexer.clone();
    //     peek.next_or_eof();
    //     peek.span().into()
    // }

    pub fn slice(&self) -> &str {
        self.lexer.slice()
    }

    pub fn slice_interned(&self) -> Spur {
        self.interner.borrow_mut().get_or_intern(self.lexer.slice())
    }

    pub fn peek(&self) -> ParseResult<Token> {
        let mut peek = self.lexer.clone();
        let mut out = peek.next_or_eof().map_err(|e| self.map_lexer_err(e))?;
        while out == Token::Newline {
            // should theoretically never be more than one, but having a loop just in case doesn't hurt
            out = peek.next_or_eof().map_err(|e| self.map_lexer_err(e))?;
        }
        Ok(out)
    }

    pub fn peek_strict(&self) -> ParseResult<Token> {
        let mut peek = self.lexer.clone();
        peek.next_or_eof().map_err(|e| self.map_lexer_err(e))
    }

    pub fn next_is(&self, tok: Token) -> ParseResult<bool> {
        Ok(self.peek()? == tok)
    }

    pub fn make_area(&self, span: CodeSpan) -> CodeArea {
        CodeArea {
            span,
            src: Rc::clone(&self.src),
        }
    }

    pub fn skip_tok(&mut self, skip: Token) -> ParseResult<bool> {
        if self.next_is(skip)? {
            self.next()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn expect_tok_named(&mut self, expect: Token, name: &str) -> ParseResult<()> {
        let next = self.next()?;
        if next != expect {
            return Err(SyntaxError::UnexpectedToken {
                found: next,
                expected: name.to_string(),
                area: self.make_area(self.span()),
            });
        }
        Ok(())
    }

    pub fn expect_tok(&mut self, expect: Token) -> Result<(), SyntaxError> {
        self.expect_tok_named(expect, expect.to_str())
    }

    pub fn next_are(&self, toks: &[Token]) -> ParseResult<bool> {
        let mut peek = self.lexer.clone();
        for tok in toks {
            if peek
                .next()
                .unwrap_or(Ok(Token::Eof))
                .map_err(|e| self.map_lexer_err(e))?
                != *tok
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn intern_string<T: AsRef<str>>(&self, string: T) -> Spur {
        self.interner.borrow_mut().get_or_intern(string)
    }

    pub fn resolve(&self, s: &Spur) -> Box<str> {
        self.interner.borrow_mut().resolve(s).into()
    }

    pub fn parse(&mut self) -> ParseResult<Ast> {
        let file_attributes = if self.next_are(&[Token::Hashtag, Token::ExclMark])? {
            self.next()?;
            self.next()?;

            self.parse_attributes::<FileAttribute>()?
        } else {
            vec![]
        };

        let statements = self.parse_statements()?;
        self.expect_tok(Token::Eof)?;

        Ok(Ast {
            statements,
            file_attributes: file_attributes.into_iter().map(|a| a.value).collect(),
        })
    }
}
