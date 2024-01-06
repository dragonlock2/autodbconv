use crate::{Database, Error};
use std::fs::File;
use std::io::Read;
use std::path::Path;

struct Tokenizer {
    data: String,
    index: usize,
}

enum TokenizerState {
    Search,
    BlockComment,
    LineComment,
    Found(usize, char),
    CharString(bool),
    Skip,
    Stop,
}

impl Tokenizer {
    fn new() -> Self {
        Self {
            data: String::new(),
            index: 0, // byte-index
        }
    }

    fn next(&mut self) -> Option<&str> {
        // search forward for start of next token
        let mut c_prev = ' ';
        let mut state = TokenizerState::Search;
        for (i, c) in self.data[self.index..].char_indices() {
            match state {
                TokenizerState::Search => {
                    if c_prev == '/' && c == '*' {
                        state = TokenizerState::BlockComment;
                    } else if c_prev == '/' && c == '/' {
                        state = TokenizerState::LineComment;
                    } else if !c.is_whitespace() && c != '/' {
                        state = TokenizerState::Found(self.index + i, c);
                        break;
                    }
                },
                TokenizerState::BlockComment => {
                    if c_prev == '*' && c == '/' {
                        state = TokenizerState::Search;
                    }
                },
                TokenizerState::LineComment => {
                    if c == '\n' {
                        state = TokenizerState::Search;
                    }
                },
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
                    },
                    TokenizerState::CharString(start) => {
                        if start {
                            state = TokenizerState::CharString(false);
                        } else if c == '"' {
                            state = TokenizerState::Stop;
                        }
                    },
                    TokenizerState::Skip => {
                        state = TokenizerState::Stop;
                    },
                    TokenizerState::Stop => {
                        state = TokenizerState::Found(start_idx + i, c);
                        break;
                    },
                    _ => (),
                }
            }
            if let TokenizerState::Found(end_idx, _) = state {
                self.index = end_idx;
            } else {
                self.index = self.data.len();
            }
            Some(&self.data[start_idx..self.index])
        } else {
            None
        }
    }
}

pub fn parse_ldf(ldf: impl AsRef<Path>) -> Result<Database, Error> {
    let mut tokens = Tokenizer::new();
    File::open(ldf)?.read_to_string(&mut tokens.data)?;

    while let Some(tok) = tokens.next() {
        // TODO parse syntax
        println!("token: '{}'", tok);
    }
    Err(Error::NotImplemented)
}
