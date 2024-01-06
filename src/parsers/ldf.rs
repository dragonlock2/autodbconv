use crate::parsers::encoding::{DatabaseType, LDFData, Signal};
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

            let new_index;
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

    fn check_equal(&mut self, expected: &[&str]) -> Result<(), Error> {
        for e in expected {
            let actual = self.next()?;
            if &actual != e {
                error!("expected: {}, actual: {}", e, actual);
                return Err(Error::IncorrectToken);
            }
        }
        Ok(())
    }
}

enum ParserState {
    Header,
    ProtocolVersion,
    LanguageVersion,
    Speed,
    ChannelName,
    Node,
    NodeComposition,
    Signal,
    DiagnosticSignal,
    Frame,
    Done,
}

pub fn parse_ldf(ldf: impl AsRef<Path>) -> Result<Database, Error> {
    let mut tokens = Tokenizer::new(ldf)?;
    let mut state = ParserState::Header;
    let mut db: Database = Default::default();
    let mut data: LDFData = Default::default();

    while !matches!(state, ParserState::Done) {
        match state {
            ParserState::Header => {
                tokens.check_equal(&["LIN_description_file", ";"])?;
                state = ParserState::ProtocolVersion;
            }
            ParserState::ProtocolVersion => {
                tokens.check_equal(&["LIN_protocol_version", "="])?;
                if tokens.next()? != LIN_VERSION_STR {
                    warn!("protocol version not {}", LIN_VERSION_STR);
                }
                tokens.check_equal(&[";"])?;
                state = ParserState::LanguageVersion;
            }
            ParserState::LanguageVersion => {
                tokens.check_equal(&["LIN_language_version", "="])?;
                if tokens.next()? != LIN_VERSION_STR {
                    warn!("language version not {}", LIN_VERSION_STR);
                }
                tokens.check_equal(&[";"])?;
                state = ParserState::Speed;
            }
            ParserState::Speed => {
                tokens.check_equal(&["LIN_speed", "="])?;
                data.bitrate = tokens.next()?.parse()?;
                data.bitrate *= 1000.0;
                tokens.check_equal(&["kbps", ";"])?;
                if tokens.peek()? == "Channel_name" {
                    state = ParserState::ChannelName;
                } else {
                    state = ParserState::Node;
                }
            }
            ParserState::ChannelName => {
                tokens.check_equal(&["Channel_name", "="])?;
                data.postfix = tokens.next()?.to_string(); // spec says indentifier, but char_string used
                tokens.check_equal(&[";"])?;
                state = ParserState::Node;
            }
            ParserState::Node => {
                tokens.check_equal(&["Nodes", "{", "Master", ":"])?;
                data.commander = tokens.next()?.to_string();
                tokens.check_equal(&[","])?;
                data.time_base = tokens.next()?.parse()?;
                tokens.check_equal(&["ms", ","])?;
                data.jitter = tokens.next()?.parse()?;
                tokens.check_equal(&["ms", ";", "Slaves", ":"])?;
                loop {
                    data.responders
                        .insert(tokens.next()?.to_string(), Vec::new());
                    let delim = tokens.next()?;
                    if delim == ";" {
                        break;
                    } else if delim != "," {
                        return Err(Error::IncorrectToken);
                    }
                }
                tokens.check_equal(&["}"])?;
                if tokens.peek()? == "composite" {
                    state = ParserState::NodeComposition;
                } else {
                    state = ParserState::Signal;
                }
            }
            ParserState::NodeComposition => {
                warn!("node composition not supported yet, ignoring section"); // TODO support?
                tokens.check_equal(&["composite", "{"])?;
                let mut depth = 1;
                while depth > 0 {
                    match tokens.next()? {
                        "{" => depth += 1,
                        "}" => depth -= 1,
                        _ => (),
                    }
                }
                state = ParserState::Signal;
            }
            ParserState::Signal => {
                tokens.check_equal(&["Signals", "{"])?;
                while tokens.peek()? != "}" {
                    let name = tokens.next()?.to_string();
                    tokens.check_equal(&[":"])?;
                    let bit_width: u16 = tokens.next()?.parse()?;
                    tokens.check_equal(&[","])?;
                    let init_value: u64;
                    if tokens.peek()? == "{" {
                        warn!("init_value_array not supported yet, defaulting to 0"); // TODO support?
                        init_value = 0;
                        while tokens.next()? != "}" {}
                    } else {
                        init_value = tokens.next()?.parse()?
                    }
                    tokens.check_equal(&[","])?;
                    let _publisher = tokens.next()?; // unused, determined by Frames field
                    while tokens.peek()? != ";" {
                        tokens.check_equal(&[","])?;
                        let subscriber = tokens.next()?;
                        if data.responders.contains_key(subscriber) {
                            data.responders
                                .get_mut(subscriber)
                                .unwrap()
                                .push(name.clone());
                        }
                    }
                    tokens.next()?; // ";"
                    db.signals.insert(
                        name,
                        Signal {
                            signed: false,
                            little_endian: true,
                            bit_start: 0, // set later
                            bit_width,
                            init_value,
                            encodings: Vec::new(),
                        },
                    );
                }
                tokens.next()?; // "}"
                if tokens.peek()? == "Diagnostic_signals" {
                    state = ParserState::DiagnosticSignal;
                } else {
                    state = ParserState::Frame;
                }
            }
            ParserState::DiagnosticSignal => {
                #[rustfmt::skip]
                tokens.check_equal(&[
                    "Diagnostic_signals", "{",
                        "MasterReqB0", ":", "8", ",", "0", ";",
                        "MasterReqB1", ":", "8", ",", "0", ";",
                        "MasterReqB2", ":", "8", ",", "0", ";",
                        "MasterReqB3", ":", "8", ",", "0", ";",
                        "MasterReqB4", ":", "8", ",", "0", ";",
                        "MasterReqB5", ":", "8", ",", "0", ";",
                        "MasterReqB6", ":", "8", ",", "0", ";",
                        "MasterReqB7", ":", "8", ",", "0", ";",
                        "SlaveRespB0", ":", "8", ",", "0", ";",
                        "SlaveRespB1", ":", "8", ",", "0", ";",
                        "SlaveRespB2", ":", "8", ",", "0", ";",
                        "SlaveRespB3", ":", "8", ",", "0", ";",
                        "SlaveRespB4", ":", "8", ",", "0", ";",
                        "SlaveRespB5", ":", "8", ",", "0", ";",
                        "SlaveRespB6", ":", "8", ",", "0", ";",
                        "SlaveRespB7", ":", "8", ",", "0", ";",
                    "}"
                ])?;
                state = ParserState::Frame;
            }
            ParserState::Frame => {
                state = ParserState::Done; // TODO rest of syntax
            }
            _ => (),
        }
    }
    db.extra = DatabaseType::LDF(data);
    Ok(db)
}
