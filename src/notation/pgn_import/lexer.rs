use std::io::BufRead;

use pgn_reader::Nag;

use crate::board::enums::BoardEnumColor;
use crate::notation::game_identity::NotationGameIdentityEnumResult;
use crate::notation::pgn_import::budget::{NotationPgnImportBudget, NotationPgnImportBudgetPolicy};
use crate::notation::pgn_import::{
    NotationPgnImportEnumLimit, NotationPgnImportEnumSyntax, NotationPgnImportLocation,
    NotationPgnImportReason, NotationPgnImportResult,
};

pub(super) enum NotationPgnImportLexerEnumToken {
    Tag { name: String, value: String },
    Move(String),
    MoveNumber { number: u32, color: BoardEnumColor },
    Nag(u8),
    Comment(String),
    BeginVariation,
    EndVariation,
    Result(NotationGameIdentityEnumResult),
}

pub(super) struct NotationPgnImportLexerToken {
    pub(super) location: NotationPgnImportLocation,
    pub(super) kind: NotationPgnImportLexerEnumToken,
}

pub(super) struct NotationPgnImportLexer<R> {
    input: R,
    location: NotationPgnImportLocation,
    token_location: NotationPgnImportLocation,
    pub(super) budget: NotationPgnImportBudget,
}

impl<R: BufRead> NotationPgnImportLexer<R> {
    pub(super) fn new(input: R, policy: NotationPgnImportBudgetPolicy) -> Self {
        Self {
            input,
            location: NotationPgnImportLocation {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            token_location: NotationPgnImportLocation {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            budget: NotationPgnImportBudget::new(policy),
        }
    }

    pub(super) fn location(&self) -> NotationPgnImportLocation {
        self.location
    }

    pub(super) fn token_location(&self) -> NotationPgnImportLocation {
        self.token_location
    }

    fn peek<C: Fn() -> bool>(
        &mut self,
        cancellation: &C,
    ) -> Result<Option<u8>, NotationPgnImportReason> {
        if cancellation() {
            return Err(NotationPgnImportReason::Cancelled);
        }
        Ok(self.input.fill_buf()?.first().copied())
    }

    fn byte<C: Fn() -> bool>(
        &mut self,
        cancellation: &C,
    ) -> Result<Option<u8>, NotationPgnImportReason> {
        let Some(byte) = self.peek(cancellation)? else {
            return Ok(None);
        };
        self.budget.input_byte()?;
        self.location.byte_offset = self
            .location
            .byte_offset
            .checked_add(1)
            .ok_or(NotationPgnImportEnumLimit::Arithmetic)?;
        if byte == b'\n' {
            self.location.line = self
                .location
                .line
                .checked_add(1)
                .ok_or(NotationPgnImportEnumLimit::Arithmetic)?;
            self.location.column = 1;
        } else {
            self.location.column = self
                .location
                .column
                .checked_add(1)
                .ok_or(NotationPgnImportEnumLimit::Arithmetic)?;
        }
        self.input.consume(1);
        Ok(Some(byte))
    }

    fn whitespace<C: Fn() -> bool>(
        &mut self,
        cancellation: &C,
    ) -> Result<(), NotationPgnImportReason> {
        loop {
            match self.peek(cancellation)? {
                Some(byte) if byte.is_ascii_whitespace() => {
                    self.byte(cancellation)?;
                }
                Some(b'%') if self.location.column == 1 => {
                    while self.peek(cancellation)?.is_some_and(|byte| byte != b'\n') {
                        self.byte(cancellation)?;
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    pub(super) fn has_next<C: Fn() -> bool>(
        &mut self,
        cancellation: &C,
    ) -> Result<bool, NotationPgnImportReason> {
        self.whitespace(cancellation)?;
        Ok(self.peek(cancellation)?.is_some())
    }

    fn push(&mut self, bytes: &mut Vec<u8>, byte: u8) -> Result<(), NotationPgnImportReason> {
        self.budget.reserve_pending_text(bytes)?;
        bytes.push(byte);
        Ok(())
    }

    fn text(bytes: Vec<u8>) -> Result<String, NotationPgnImportReason> {
        let text = String::from_utf8(bytes).map_err(|error| error.utf8_error())?;
        if text.contains('\0') {
            return Err(NotationPgnImportEnumSyntax::InvalidText.into());
        }
        Ok(text)
    }

    fn comment<C: Fn() -> bool>(
        &mut self,
        is_brace: bool,
        cancellation: &C,
    ) -> Result<String, NotationPgnImportReason> {
        let mut bytes = Vec::new();
        loop {
            match self.byte(cancellation)? {
                Some(b'}') if is_brace => return Self::text(bytes),
                Some(b'\r' | b'\n') if !is_brace => return Self::text(bytes),
                Some(byte) => self.push(&mut bytes, byte)?,
                None if !is_brace => return Self::text(bytes),
                None => return Err(NotationPgnImportEnumSyntax::UnterminatedComment.into()),
            }
        }
    }

    fn tag<C: Fn() -> bool>(
        &mut self,
        cancellation: &C,
    ) -> Result<NotationPgnImportLexerEnumToken, NotationPgnImportReason> {
        let mut name = Vec::new();
        while self
            .peek(cancellation)?
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            let byte = self
                .byte(cancellation)?
                .ok_or(NotationPgnImportEnumSyntax::UnterminatedTag)?;
            self.push(&mut name, byte)?;
        }
        if name.is_empty()
            || !self
                .peek(cancellation)?
                .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            return Err(NotationPgnImportEnumSyntax::InvalidTag.into());
        }
        self.tag_whitespace(cancellation)?;
        if self.byte(cancellation)? != Some(b'"') {
            return Err(NotationPgnImportEnumSyntax::InvalidTag.into());
        }
        let mut value = Vec::new();
        loop {
            match self.byte(cancellation)? {
                Some(b'"') => break,
                Some(b'\\') => {
                    let escaped = self
                        .byte(cancellation)?
                        .ok_or(NotationPgnImportEnumSyntax::UnterminatedTag)?;
                    if !matches!(escaped, b'\\' | b'"') {
                        return Err(NotationPgnImportEnumSyntax::InvalidEscape.into());
                    }
                    self.push(&mut value, escaped)?;
                }
                Some(b'\r' | b'\n') => return Err(NotationPgnImportEnumSyntax::InvalidTag.into()),
                Some(byte) => {
                    self.push(&mut value, byte)?;
                }
                None => return Err(NotationPgnImportEnumSyntax::UnterminatedTag.into()),
            }
        }
        self.tag_whitespace(cancellation)?;
        if self.byte(cancellation)? != Some(b']') {
            return Err(NotationPgnImportEnumSyntax::InvalidTag.into());
        }
        Ok(NotationPgnImportLexerEnumToken::Tag {
            name: Self::text(name)?,
            value: Self::text(value)?,
        })
    }

    fn tag_whitespace<C: Fn() -> bool>(
        &mut self,
        cancellation: &C,
    ) -> Result<(), NotationPgnImportReason> {
        while self
            .peek(cancellation)?
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            self.byte(cancellation)?;
        }
        Ok(())
    }

    fn is_delimiter(byte: u8) -> bool {
        byte.is_ascii_whitespace()
            || matches!(
                byte,
                b'(' | b')' | b'{' | b'}' | b'[' | b']' | b';' | b'$' | b'!' | b'?' | b'.' | b'*'
            )
    }

    fn glyph<C: Fn() -> bool>(
        &mut self,
        first: u8,
        cancellation: &C,
    ) -> Result<NotationPgnImportLexerEnumToken, NotationPgnImportReason> {
        let mut bytes = Vec::new();
        self.push(&mut bytes, first)?;
        while self.peek(cancellation)?.is_some_and(|byte| {
            if first == b'$' {
                !Self::is_delimiter(byte)
            } else {
                matches!(byte, b'!' | b'?')
            }
        }) {
            let byte = self
                .byte(cancellation)?
                .ok_or(NotationPgnImportEnumSyntax::InvalidNag)?;
            self.push(&mut bytes, byte)?;
        }
        if first == b'$' && (bytes.len() < 2 || !bytes[1..].iter().all(u8::is_ascii_digit)) {
            return Err(NotationPgnImportEnumSyntax::InvalidNag.into());
        }
        let nag =
            Nag::from_ascii(&bytes).map_err(|_error| NotationPgnImportEnumSyntax::InvalidNag)?;
        Ok(NotationPgnImportLexerEnumToken::Nag(nag.0))
    }

    fn word<C: Fn() -> bool>(
        &mut self,
        first: u8,
        cancellation: &C,
    ) -> Result<NotationPgnImportLexerEnumToken, NotationPgnImportReason> {
        let mut bytes = Vec::new();
        self.push(&mut bytes, first)?;
        while self
            .peek(cancellation)?
            .is_some_and(|byte| !Self::is_delimiter(byte))
        {
            let byte = self
                .byte(cancellation)?
                .ok_or(NotationPgnImportEnumSyntax::UnexpectedToken)?;
            self.push(&mut bytes, byte)?;
        }
        let word = Self::text(bytes)?;
        if word.bytes().all(|byte| byte.is_ascii_digit()) && self.peek(cancellation)? == Some(b'.')
        {
            let mut dots = 0_u8;
            while self.peek(cancellation)? == Some(b'.') {
                self.byte(cancellation)?;
                dots = dots
                    .checked_add(1)
                    .ok_or(NotationPgnImportEnumSyntax::InvalidMoveNumber)?;
            }
            let color = match dots {
                1 => BoardEnumColor::White,
                3 => BoardEnumColor::Black,
                _ => return Err(NotationPgnImportEnumSyntax::InvalidMoveNumber.into()),
            };
            let number = word
                .parse()
                .map_err(|_error| NotationPgnImportEnumSyntax::InvalidMoveNumber)?;
            return Ok(NotationPgnImportLexerEnumToken::MoveNumber { number, color });
        }
        if let Some(result) = NotationPgnImportResult::parse(&word) {
            return Ok(NotationPgnImportLexerEnumToken::Result(result));
        }
        self.budget.ply()?;
        Ok(NotationPgnImportLexerEnumToken::Move(word))
    }

    pub(super) fn next<C: Fn() -> bool>(
        &mut self,
        cancellation: &C,
    ) -> Result<Option<NotationPgnImportLexerToken>, NotationPgnImportReason> {
        self.budget.clear_pending();
        self.whitespace(cancellation)?;
        let location = self.location;
        self.token_location = location;
        let Some(first) = self.byte(cancellation)? else {
            return Ok(None);
        };
        let kind = match first {
            b'[' => self.tag(cancellation)?,
            b'{' => NotationPgnImportLexerEnumToken::Comment(self.comment(true, cancellation)?),
            b';' => NotationPgnImportLexerEnumToken::Comment(self.comment(false, cancellation)?),
            b'(' => NotationPgnImportLexerEnumToken::BeginVariation,
            b')' => NotationPgnImportLexerEnumToken::EndVariation,
            b'*' => {
                NotationPgnImportLexerEnumToken::Result(NotationGameIdentityEnumResult::Unfinished)
            }
            b'$' | b'!' | b'?' => self.glyph(first, cancellation)?,
            b'}' | b']' | b'.' | b'"' => {
                return Err(NotationPgnImportEnumSyntax::UnexpectedToken.into());
            }
            byte => self.word(byte, cancellation)?,
        };
        Ok(Some(NotationPgnImportLexerToken { location, kind }))
    }
}
