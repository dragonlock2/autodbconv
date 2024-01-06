use crate::parsers::encoding::LDFData;
use crate::{Database, Error};
use log::{error, warn};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const LIN_VERSION_STR: &str = "\"2.2\"";

struct Tokenizer {
    data: String,
    index: usize,
}

enum TokenizerState {
    Search,
    ExpectComment,
    BlockComment,
    LineComment,
    CharString(bool),
    Skip,
    Stop,
    Found(usize, char),
}

impl Tokenizer {
    fn new(file: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let mut ret = Self {
            data: String::new(),
            index: 0, // byte-index
        };
        File::open(file)?.read_to_string(&mut ret.data)?;
        Ok(ret)
    }

    fn parse(&mut self, update: bool) -> Result<&str, Error> {
        // search forward for start of next token
        let mut c_prev = ' ';
        let mut state = TokenizerState::Search;
        for (i, c) in self.data[self.index..].char_indices() {
            match state {
                TokenizerState::Search => {
                    if c == '/' {
                        state = TokenizerState::ExpectComment;
                    } else if !c.is_whitespace() {
                        state = TokenizerState::Found(self.index + i, c);
                        break;
                    }
                }
                TokenizerState::ExpectComment => {
                    if c == '*' {
                        state = TokenizerState::BlockComment;
                    } else if c == '/' {
                        state = TokenizerState::LineComment;
                    } else {
                        return Err(Error::ExpectedComment);
                    }
                }
                TokenizerState::BlockComment => {
                    if c_prev == '*' && c == '/' {
                        state = TokenizerState::Search;
                    }
                }
                TokenizerState::LineComment => {
                    if c == '\n' {
                        state = TokenizerState::Search;
                    }
                }
                _ => (),
            }
            c_prev = c;
        }

        // find end of token, update index
        let is_delimiter = |c: char| [',', ';', ':', '=', '{', '}', '/'].contains(&c);
        if let TokenizerState::Found(start_idx, c_start) = state {
            if let '"' = c_start {
                state = TokenizerState::CharString(true);
            } else if is_delimiter(c_start) {
                state = TokenizerState::Skip;
            } else {
                state = TokenizerState::Search;
            }
            for (i, c) in self.data[start_idx..].char_indices() {
                match state {
                    TokenizerState::Search => {
                        if is_delimiter(c) || c.is_whitespace() {
                            state = TokenizerState::Found(start_idx + i, c);
                            break;
                        }
                    }
                    TokenizerState::CharString(start) => {
                        if start {
                            state = TokenizerState::CharString(false);
                        } else if c == '"' {
                            state = TokenizerState::Stop;
                        }
                    }
                    TokenizerState::Skip => {
                        state = TokenizerState::Stop;
                    }
                    TokenizerState::Stop => {
                        state = TokenizerState::Found(start_idx + i, c);
                        break;
                    }
                    _ => (),
                }
            }

            let mut new_index = 0;
            if let TokenizerState::Found(end_idx, _) = state {
                new_index = end_idx;
            } else {
                new_index = self.data.len();
            }
            if update {
                self.index = new_index;
            }
            Ok(&self.data[start_idx..new_index])
        } else {
            Err(Error::ExpectedToken)
        }
    }

    fn next(&mut self) -> Result<&str, Error> {
        self.parse(true)
    }

    fn peek(&mut self) -> Result<&str, Error> {
        self.parse(false)
    }
}

fn check_equal(actual: &str, expected: &str) -> Result<(), Error> {
    if actual == expected {
        Ok(())
    } else {
        error!("expected: {}, actual: {}", expected, actual);
        Err(Error::IncorrectToken)
    }
}

enum ParserState {
    Header,
    ProtocolVersion,
    LanguageVersion,
    Speed,
    ChannelName,
    NodeDef,
    Done,
}

pub fn parse_ldf(ldf: impl AsRef<Path>) -> Result<Database, Error> {
    let mut tokens = Tokenizer::new(ldf)?;
    let mut state = ParserState::Header;

    let mut data = LDFData {
        bitrate: 0.0,
        postfix: String::new(),
    };

    while !matches!(state, ParserState::Done) {
        match state {
            ParserState::Header => {
                check_equal(tokens.next()?, "LIN_description_file")?;
                check_equal(tokens.next()?, ";")?;
                state = ParserState::ProtocolVersion;
            }
            ParserState::ProtocolVersion => {
                check_equal(tokens.next()?, "LIN_protocol_version")?;
                check_equal(tokens.next()?, "=")?;
                if tokens.next()? != LIN_VERSION_STR {
                    warn!("protocol version not {}", LIN_VERSION_STR);
                }
                check_equal(tokens.next()?, ";")?;
                state = ParserState::LanguageVersion;
            }
            ParserState::LanguageVersion => {
                check_equal(tokens.next()?, "LIN_language_version")?;
                check_equal(tokens.next()?, "=")?;
                if tokens.next()? != LIN_VERSION_STR {
                    warn!("language version not {}", LIN_VERSION_STR);
                }
                check_equal(tokens.next()?, ";")?;
                state = ParserState::Speed;
            }
            ParserState::Speed => {
                check_equal(tokens.next()?, "LIN_speed")?;
                check_equal(tokens.next()?, "=")?;
                data.bitrate = tokens.next()?.parse()?;
                data.bitrate *= 1000.0;
                check_equal(tokens.next()?, "kbps")?;
                check_equal(tokens.next()?, ";")?;
                if tokens.peek()? == "Channel_name" {
                    state = ParserState::ChannelName;
                } else {
                    state = ParserState::NodeDef;
                }
            }
            ParserState::ChannelName => {
                check_equal(tokens.next()?, "Channel_name")?;
                check_equal(tokens.next()?, "=")?;
                data.postfix += tokens.next()?; // spec says indentifier, but char_string used
                check_equal(tokens.next()?, ";")?;
                state = ParserState::NodeDef;
            }
            ParserState::NodeDef => {
                state = ParserState::Done; // TODO rest of syntax
            }
            _ => (),
        }
    }

    // TODO assemble database
    dbg!(data);
    Err(Error::NotImplemented)
}
