use crate::parsers::encoding::{
    DatabaseType, Encoding, LDFData, LDFScheduleCommand, Message, Signal, BIT_START_INVALID,
    MAX_SIGNAL_WIDTH,
};
use crate::{Database, Error};
use log::{error, warn};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

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
    SporadicFrame,
    EventTriggeredFrame,
    DiagnosticFrame,
    NodeAttributes,
    ScheduleTable,
    SignalGroups,
    SignalEncodingTypes,
    SignalRepresentation,
    Done,
}

fn parse_real_or_integer(s: &str) -> Result<f64, <f64 as FromStr>::Err> {
    if s.starts_with("0x") {
        if let Ok(i) = u64::from_str_radix(&s[2..], 16) {
            Ok(i as f64)
        } else {
            "z".parse() // create ParseFloatError
        }
    } else {
        s.parse()
    }
}

fn parse_integer(s: &str) -> Result<u64, <u64 as FromStr>::Err> {
    if s.starts_with("0x") {
        u64::from_str_radix(&s[2..], 16)
    } else {
        s.parse()
    }
}

pub fn parse_ldf(ldf: impl AsRef<Path>) -> Result<Database, Error> {
    let mut tokens = Tokenizer::new(ldf)?;
    let mut state = ParserState::Header;
    let mut db: Database = Default::default();
    let mut data: LDFData = Default::default();
    let mut encodings: HashMap<String, Vec<Encoding>> = HashMap::new();

    // first pass parse data
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
                data.bitrate = parse_real_or_integer(tokens.next()?)?;
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
                data.time_base = parse_real_or_integer(tokens.next()?)?;
                tokens.check_equal(&["ms", ","])?;
                data.jitter = parse_real_or_integer(tokens.next()?)?;
                tokens.check_equal(&["ms", ";", "Slaves", ":"])?;
                loop {
                    data.responders
                        .insert(tokens.next()?.to_string(), Default::default());
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
                    let bit_width = parse_integer(tokens.next()?)? as u16;
                    if bit_width > MAX_SIGNAL_WIDTH {
                        return Err(Error::SignalTooWide);
                    }
                    tokens.check_equal(&[","])?;
                    let init_value;
                    if tokens.peek()? == "{" {
                        warn!("init_value_array not supported yet, defaulting to 0"); // TODO support?
                        init_value = 0;
                        while tokens.next()? != "}" {}
                    } else {
                        init_value = parse_integer(tokens.next()?)?;
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
                                .subscribed_signals
                                .push(name.clone());
                        }
                    }
                    tokens.next()?; // ";"
                    db.signals.insert(
                        name,
                        Signal {
                            signed: false,
                            little_endian: true,
                            bit_start: BIT_START_INVALID, // set later
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
                tokens.check_equal(&["Frames", "{"])?;
                while tokens.peek()? != "}" {
                    let name = tokens.next()?.to_string();
                    tokens.check_equal(&[":"])?;
                    let id = parse_integer(tokens.next()?)? as u32;
                    tokens.check_equal(&[","])?;
                    let sender = tokens.next()?.to_string();
                    tokens.check_equal(&[","])?;
                    let byte_width = parse_integer(tokens.next()?)? as u16;
                    tokens.check_equal(&["{"])?;
                    let mut signals = Vec::new();
                    while tokens.peek()? != "}" {
                        let signal_name = tokens.next()?.to_string();
                        tokens.check_equal(&[","])?;
                        let signal_offset = parse_integer(tokens.next()?)? as u16;
                        tokens.check_equal(&[";"])?;
                        if db.signals.contains_key(&signal_name) {
                            if db.signals[&signal_name].bit_start == BIT_START_INVALID {
                                db.signals.get_mut(&signal_name).unwrap().bit_start = signal_offset;
                            } else {
                                return Err(Error::DuplicateSignal);
                            }
                        } else {
                            return Err(Error::UnknownSignal);
                        }
                        signals.push(signal_name);
                    }
                    tokens.next()?; // "}"
                    db.messages.insert(
                        name,
                        Message {
                            sender,
                            id,
                            byte_width,
                            signals,
                            mux_signals: HashMap::new(), // none
                        },
                    );
                }
                tokens.next()?; // "}"
                match tokens.peek()? {
                    "Sporadic_frames" => state = ParserState::SporadicFrame,
                    "Event_triggered_frames" => state = ParserState::EventTriggeredFrame,
                    "Diagnostic_frames" => state = ParserState::DiagnosticFrame,
                    _ => state = ParserState::NodeAttributes,
                }
            }
            ParserState::SporadicFrame => {
                tokens.check_equal(&["Sporadic_frames", "{"])?;
                while tokens.peek()? != "}" {
                    let name = tokens.next()?.to_string();
                    tokens.check_equal(&[":"])?;
                    let mut frames = vec![tokens.next()?.to_string()]; // at least one frame
                    while tokens.peek()? != ";" {
                        tokens.check_equal(&[","])?;
                        let f = tokens.next()?.to_string();
                        if !db.messages.contains_key(&f) {
                            return Err(Error::UnknownFrame);
                        } else if db.messages[&f].sender != data.commander {
                            return Err(Error::SporadicFrameHasResponder);
                        } else if frames.contains(&f) {
                            return Err(Error::DuplicateFrame);
                        }
                        frames.push(f);
                    }
                    tokens.next()?; // ";"
                    if db.messages.contains_key(&name) || data.sporadic_frames.contains_key(&name) {
                        return Err(Error::DuplicateFrame);
                    } else {
                        data.sporadic_frames.insert(name, frames);
                    }
                }
                tokens.next()?; // "}"
                match tokens.peek()? {
                    "Event_triggered_frames" => state = ParserState::EventTriggeredFrame,
                    "Diagnostic_frames" => state = ParserState::DiagnosticFrame,
                    _ => state = ParserState::NodeAttributes,
                }
            }
            ParserState::EventTriggeredFrame => {
                tokens.check_equal(&["Event_triggered_frames", "{"])?;
                while tokens.peek()? != "}" {
                    let name = tokens.next()?.to_string();
                    tokens.check_equal(&[":"])?;
                    let resolver = tokens.next()?.to_string();
                    tokens.check_equal(&[","])?;
                    let id = parse_integer(tokens.next()?)? as u32;
                    let mut frames = Vec::new();
                    while tokens.peek()? != ";" {
                        tokens.check_equal(&[","])?;
                        let f = tokens.next()?.to_string();
                        if frames.contains(&f) {
                            return Err(Error::DuplicateFrame);
                        } else if db.messages.contains_key(&f) {
                            frames.push(f);
                        } else {
                            return Err(Error::NotUnconditionalFrame);
                        }
                    }
                    tokens.next()?; // ";"
                    let all_same_len;
                    if frames.is_empty() {
                        all_same_len = true;
                    } else {
                        let first = db.messages[&frames[0]].byte_width;
                        all_same_len = frames.iter().all(|f| db.messages[f].byte_width == first);
                    }
                    if db.messages.contains_key(&name)
                        || data.sporadic_frames.contains_key(&name)
                        || data.event_frames.contains_key(&name)
                    {
                        return Err(Error::DuplicateFrame);
                    } else if all_same_len {
                        data.event_frames.insert(name, (resolver, id, frames));
                    } else {
                        return Err(Error::EventFrameDifferentLength);
                    }
                }
                tokens.next()?; // "}"
                match tokens.peek()? {
                    "Diagnostic_frames" => state = ParserState::DiagnosticFrame,
                    _ => state = ParserState::NodeAttributes,
                }
            }
            ParserState::DiagnosticFrame => {
                #[rustfmt::skip]
                tokens.check_equal(&[
                    "Diagnostic_frames", "{",
                        "MasterReq", ":", "60", "{",
                            "MasterReqB0", ",", "0", ";",
                            "MasterReqB1", ",", "8", ";",
                            "MasterReqB2", ",", "16", ";",
                            "MasterReqB3", ",", "24", ";",
                            "MasterReqB4", ",", "32", ";",
                            "MasterReqB5", ",", "40", ";",
                            "MasterReqB6", ",", "48", ";",
                            "MasterReqB7", ",", "56", ";",
                        "}",
                        "SlaveResp", ":", "61", "{",
                            "SlaveRespB0", ",", "0", ";",
                            "SlaveRespB1", ",", "8", ";",
                            "SlaveRespB2", ",", "16", ";",
                            "SlaveRespB3", ",", "24", ";",
                            "SlaveRespB4", ",", "32", ";",
                            "SlaveRespB5", ",", "40", ";",
                            "SlaveRespB6", ",", "48", ";",
                            "SlaveRespB7", ",", "56", ";",
                        "}",
                    "}"
                ])?;
                state = ParserState::NodeAttributes;
            }
            ParserState::NodeAttributes => {
                tokens.check_equal(&["Node_attributes", "{"])?;
                while tokens.peek()? != "}" {
                    let name = tokens.next()?.to_string();
                    if !data.responders.contains_key(&name) {
                        return Err(Error::UnknownNode);
                    }
                    let resp = data.responders.get_mut(&name).unwrap();
                    tokens.check_equal(&["{", "LIN_protocol", "="])?;
                    let protocol = tokens.next()?.to_string();
                    tokens.check_equal(&[";", "configured_NAD", "="])?;
                    resp.configured_nad = parse_integer(tokens.next()?)? as u8;
                    tokens.check_equal(&[";"])?;
                    if tokens.peek()? == "initial_NAD" {
                        tokens.check_equal(&["initial_NAD", "="])?;
                        resp.initial_nad = Some(parse_integer(tokens.next()?)? as u8);
                        tokens.check_equal(&[";"])?;
                    }
                    if protocol.starts_with("\"2.") {
                        tokens.check_equal(&["product_id", "="])?;
                        let supplier_id = parse_integer(tokens.next()?)? as u16;
                        tokens.check_equal(&[","])?;
                        let function_id = parse_integer(tokens.next()?)? as u16;
                        let variant;
                        if tokens.peek()? == "," {
                            tokens.next()?; // ","
                            variant = parse_integer(tokens.next()?)? as u8;
                        } else {
                            variant = 0;
                        }
                        resp.product_id = Some((supplier_id, function_id, variant));
                        tokens.check_equal(&[";", "response_error", "="])?;
                        let response_error = tokens.next()?.to_string();
                        if db.signals.contains_key(&response_error) {
                            resp.response_error = Some(response_error);
                        } else {
                            return Err(Error::UnknownSignal);
                        }
                        tokens.check_equal(&[";"])?;
                        for s in [
                            "fault_state_signals",
                            "P2_min",
                            "ST_min",
                            "N_As_timeout",
                            "N_Cr_timeout",
                        ] {
                            if tokens.peek()? == s {
                                warn!("{} not supported yet, ignoring", s); // TODO support?
                                tokens.check_equal(&[s, "="])?;
                                while tokens.next()? != ";" {}
                            }
                        }
                        tokens.check_equal(&["configurable_frames", "{"])?;
                        while tokens.peek()? != "}" {
                            let frame = tokens.next()?.to_string();
                            if !db.messages.contains_key(&frame)
                                && !data.event_frames.contains_key(&frame)
                            {
                                return Err(Error::UnknownFrame);
                            }
                            let id;
                            if tokens.peek()? == "=" {
                                tokens.next()?; // "="
                                id = Some(parse_integer(tokens.next()?)? as u16);
                            } else {
                                id = None;
                            }
                            tokens.check_equal(&[";"])?;
                            resp.configurable_frames.push((frame, id));
                        }
                        tokens.next()?; // "}"
                    }
                    tokens.next()?; // "}"
                }
                tokens.next()?; // "}"
                state = ParserState::ScheduleTable;
            }
            ParserState::ScheduleTable => {
                tokens.check_equal(&["Schedule_tables", "{"])?;
                while tokens.peek()? != "}" {
                    let name = tokens.next()?.to_string();
                    let mut table = Vec::new();
                    tokens.check_equal(&["{"])?;
                    while tokens.peek()? != "}" {
                        let cmd = tokens.next()?.to_string();
                        let command;
                        match cmd.as_str() {
                            "MasterReq" => command = LDFScheduleCommand::CommanderReq,
                            "SlaveResp" => command = LDFScheduleCommand::ResponderResp,
                            "AssignNAD" => {
                                tokens.check_equal(&["{"])?;
                                let node = tokens.next()?.to_string();
                                if !data.responders.contains_key(&node) {
                                    return Err(Error::UnknownNode);
                                }
                                tokens.check_equal(&["}"])?;
                                command = LDFScheduleCommand::AssignNAD(node);
                            }
                            "ConditionalChangeNAD" => {
                                tokens.check_equal(&["{"])?;
                                let mut fields = [0; 6];
                                for i in 0..fields.len() {
                                    fields[i] = parse_integer(tokens.next()?)? as u8;
                                    if i != fields.len() - 1 {
                                        tokens.check_equal(&[","])?;
                                    } else {
                                        tokens.check_equal(&["}"])?;
                                    }
                                }
                                command = LDFScheduleCommand::ConditionalChangeNAD {
                                    nad: fields[0],
                                    id: fields[1],
                                    byte: fields[2],
                                    mask: fields[3],
                                    inv: fields[4],
                                    new_nad: fields[5],
                                };
                            }
                            "DataDump" => {
                                tokens.check_equal(&["{"])?;
                                let node = tokens.next()?.to_string();
                                if !data.responders.contains_key(&node) {
                                    return Err(Error::UnknownNode);
                                }
                                tokens.check_equal(&[","])?;
                                let mut d = [0; 5];
                                for i in 0..d.len() {
                                    d[i] = parse_integer(tokens.next()?)? as u8;
                                    if i != d.len() - 1 {
                                        tokens.check_equal(&[","])?;
                                    } else {
                                        tokens.check_equal(&["}"])?;
                                    }
                                }
                                command = LDFScheduleCommand::DataDump {
                                    name: node,
                                    data: d,
                                };
                            }
                            "SaveConfiguration" => {
                                tokens.check_equal(&["{"])?;
                                let node = tokens.next()?.to_string();
                                if !data.responders.contains_key(&node) {
                                    return Err(Error::UnknownNode);
                                }
                                tokens.check_equal(&["}"])?;
                                command = LDFScheduleCommand::SaveConfiguration(node);
                            }
                            "AssignFrameIdRange" => {
                                tokens.check_equal(&["{"])?;
                                let node = tokens.next()?.to_string();
                                if !data.responders.contains_key(&node) {
                                    return Err(Error::UnknownNode);
                                }
                                tokens.check_equal(&[","])?;
                                let index = parse_integer(tokens.next()?)? as u8;
                                let mut pid = [0; 4];
                                if tokens.peek()? == "," {
                                    tokens.next()?; // ","
                                    for i in 0..pid.len() {
                                        pid[i] = parse_integer(tokens.next()?)? as u8;
                                        if i != pid.len() - 1 {
                                            tokens.check_equal(&[","])?;
                                        } else {
                                            tokens.check_equal(&["}"])?;
                                        }
                                    }
                                } else {
                                    warn!("getting PID from configurable_frames not supported yet, default to 0xFF"); // TODO support?
                                    pid = [0xFF, 0xFF, 0xFF, 0xFF];
                                    tokens.check_equal(&["}"])?;
                                }
                                command = LDFScheduleCommand::AssignFrameIdRange {
                                    name: node,
                                    index,
                                    pid,
                                };
                            }
                            "FreeFormat" => {
                                tokens.check_equal(&["{"])?;
                                let mut d = [0; 8];
                                for i in 0..d.len() {
                                    d[i] = parse_integer(tokens.next()?)? as u8;
                                    if i != d.len() - 1 {
                                        tokens.check_equal(&[","])?;
                                    } else {
                                        tokens.check_equal(&["}"])?;
                                    }
                                }
                                command = LDFScheduleCommand::FreeFormat(d);
                            }
                            "AssignFrameId" => {
                                tokens.check_equal(&["{"])?;
                                let node = tokens.next()?.to_string();
                                if !data.responders.contains_key(&node) {
                                    return Err(Error::UnknownNode);
                                }
                                tokens.check_equal(&[","])?;
                                let frame = tokens.next()?.to_string();
                                if !db.messages.contains_key(&frame) {
                                    return Err(Error::UnknownFrame);
                                }
                                tokens.check_equal(&["}"])?;
                                command = LDFScheduleCommand::AssignFrameId { node, frame };
                            }
                            _ => {
                                if !db.messages.contains_key(&cmd)
                                    && !data.sporadic_frames.contains_key(&cmd)
                                    && !data.event_frames.contains_key(&cmd)
                                {
                                    return Err(Error::UnknownFrame);
                                }
                                command = LDFScheduleCommand::Frame(cmd);
                            }
                        }
                        tokens.check_equal(&["delay"])?;
                        let frame_time = parse_real_or_integer(tokens.next()?)?;
                        tokens.check_equal(&["ms", ";"])?;
                        table.push((command, frame_time));
                    }
                    tokens.next()?; // "}"
                    data.schedule_tables.insert(name, table);
                }
                tokens.next()?; // "}"
                if let Ok(tok) = tokens.peek() {
                    match tok {
                        "Signal_groups" => state = ParserState::SignalGroups,
                        "Signal_encoding_types" => state = ParserState::SignalEncodingTypes,
                        "Signal_representation" => state = ParserState::SignalRepresentation,
                        _ => return Err(Error::UnexpectedToken),
                    }
                } else {
                    state = ParserState::Done;
                }
            }
            ParserState::SignalGroups => {
                warn!("signal groups deprecated, ignoring section");
                tokens.check_equal(&["Signal_groups", "{"])?;
                let mut depth = 1;
                while depth > 0 {
                    match tokens.next()? {
                        "{" => depth += 1,
                        "}" => depth -= 1,
                        _ => (),
                    }
                }
                if let Ok(tok) = tokens.peek() {
                    match tok {
                        "Signal_encoding_types" => state = ParserState::SignalEncodingTypes,
                        "Signal_representation" => state = ParserState::SignalRepresentation,
                        _ => return Err(Error::UnexpectedToken),
                    }
                } else {
                    state = ParserState::Done;
                }
            }
            ParserState::SignalEncodingTypes => {
                tokens.check_equal(&["Signal_encoding_types", "{"])?;
                while tokens.peek()? != "}" {
                    let name = tokens.next()?.to_string();
                    if encodings.contains_key(&name) {
                        return Err(Error::DuplicateEncoding);
                    }
                    encodings.insert(name.clone(), Vec::new());
                    tokens.check_equal(&["{"])?;
                    let mut logicals = HashMap::new();
                    while tokens.peek()? != "}" {
                        match tokens.next()? {
                            "logical_value" => {
                                tokens.check_equal(&[","])?;
                                let val = parse_integer(tokens.next()?)?;
                                if tokens.peek()? == "," {
                                    tokens.next()?; // ","
                                    logicals.insert(tokens.next()?.to_string(), val);
                                } else {
                                    warn!("logical value w/o text, ignoring"); // opinionated take :)
                                }
                            }
                            "physical_value" => {
                                tokens.check_equal(&[","])?;
                                let raw_min = parse_integer(tokens.next()?)?;
                                tokens.check_equal(&[","])?;
                                let raw_max = parse_integer(tokens.next()?)?;
                                tokens.check_equal(&[","])?;
                                let scale = parse_real_or_integer(tokens.next()?)?;
                                tokens.check_equal(&[","])?;
                                let offset = parse_real_or_integer(tokens.next()?)?;
                                let unit;
                                if tokens.peek()? == "," {
                                    tokens.next()?; // ","
                                    unit = tokens.next()?.to_string();
                                } else {
                                    unit = "".to_string();
                                }
                                encodings.get_mut(&name).unwrap().push(Encoding::Scalar {
                                    raw_min,
                                    raw_max,
                                    scale,
                                    offset,
                                    unit,
                                });
                            }
                            "bcd_value" => {
                                warn!("bcd encoding not supported, ignoring");
                            }
                            "ascii_value" => {
                                warn!("ascii encoding not supported, ignoring");
                            }
                            _ => return Err(Error::IncorrectToken),
                        }
                        tokens.check_equal(&[";"])?;
                    }
                    tokens.next()?; // "}"
                    if !logicals.is_empty() {
                        encodings.get_mut(&name).unwrap().push(Encoding::Enum {
                            name: name,
                            map: logicals,
                        });
                    }
                }
                tokens.next()?; // "}"
                if let Ok(tok) = tokens.peek() {
                    match tok {
                        "Signal_representation" => state = ParserState::SignalRepresentation,
                        _ => return Err(Error::UnexpectedToken),
                    }
                } else {
                    state = ParserState::Done;
                }
            }
            ParserState::SignalRepresentation => {
                error!("TODO signal representation"); // TODO parse
                if tokens.peek().is_ok() {
                    return Err(Error::UnexpectedToken);
                }
                state = ParserState::Done;
            }
            _ => (),
        }
    }

    // TODO second pass validation
    /*
     * - no signal in frame overlap and fit in width (make generic db validate function)
     * - no message id overlap, include event triggered frames (use db validate)
     * - event triggered frames have first byte free
     * - resolver schedule tables exist, no event triggered frames in it!
     * - no event triggered frames and associated frame in same schedule table
     */
    db.extra = DatabaseType::LDF(data);
    Ok(db)
}
