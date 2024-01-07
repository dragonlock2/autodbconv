use std::collections::HashMap;

pub const MAX_SIGNAL_WIDTH: u16 = 64;
pub const BIT_START_INVALID: u16 = u16::MAX;

#[derive(Debug)]
pub enum Encoding {
    Scalar {
        raw_min: u64,
        raw_max: u64,
        scale: f64,
        offset: f64, // actual = scale * raw + offset
        unit: String,
    },
    Enum {
        name: String,
        map: HashMap<String, u32>,
    },
}

/*
 * Allocation with mixed endian can get confusing. Here's an example mask for an 8-bit signal across 2 bytes.
 *  little - bit_start=4, bit_width=8, F0 0F
 *  big    - bit_start=3, bit_width=8, 0F F0
 *
 * Little-endian counts up as expected since bit_start encodes the LSB, but big-endian counts down in a sawtooth
 * pattern since bit_start encodes the MSB.
 */
#[derive(Debug)]
pub struct Signal {
    pub signed: bool,
    pub little_endian: bool,
    pub bit_start: u16,
    pub bit_width: u16,
    pub init_value: u64,
    pub encodings: Vec<Encoding>,
}

#[derive(Debug)]
pub struct Message {
    pub sender: String,
    pub id: u32,
    pub byte_width: u16,
    pub signals: Vec<String>,
    pub mux_signals: HashMap<String, (u64, Vec<String>)>,
}

#[derive(Debug, Default)]
pub struct LDFData {
    pub bitrate: f64, // bps
    pub postfix: String,
    pub commander: String,
    pub time_base: f64,                           // ms
    pub jitter: f64,                              // ms
    pub responders: HashMap<String, Vec<String>>, // node => subscribed signals
    pub sporadic_frames: HashMap<String, Vec<String>>,
    pub event_frames: HashMap<String, (String, u32, Vec<String>)>, // collision resolver, id, list of frames
                                                                   // TODO schedule tables
                                                                   // TODO NADs?
}

#[derive(Debug)]
pub enum DatabaseType {
    NCF,
    LDF(LDFData),
    DBC,
}

#[derive(Debug, Default)]
pub struct Database {
    pub signals: HashMap<String, Signal>,
    pub messages: HashMap<String, Message>,
    pub extra: DatabaseType,
}

impl Default for DatabaseType {
    fn default() -> Self {
        DatabaseType::NCF
    }
}
