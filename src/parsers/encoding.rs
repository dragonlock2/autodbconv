use std::collections::HashMap;

const MAX_SIGNAL_WIDTH: u16 = 64;

enum Encoding {
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

/**
 * Allocation with mixed endian can get confusing. Here's an example mask for an 8-bit signal across 2 bytes.
 *  little - bit_start=4, bit_width=8, F0 0F
 *  big    - bit_start=3, bit_width=8, 0F F0
 *
 * Little-endian counts up as expected since bit_start encodes the LSB, but big-endian counts down in a sawtooth
 * pattern since bit_start encodes the MSB.
 */
struct Signal {
    signed: bool,
    little_endian: bool,
    bit_start: u16,
    bit_width: u16,
    default_value: u64,
    encodings: Vec<Encoding>,
}

struct Message {
    sender: String,
    id: u32,
    byte_width: u16,
    signals: Vec<String>,
    mux_signals: HashMap<String, (u64, Vec<String>)>,
}

enum DatabaseType {
    NCF {
        // TODO similar to LDF
    },
    LDF {
        bus_speed: u32,
        // TODO schedule tables
        // TODO NADs?
    },
    DBC {
        bus_speed: u32,
    },
}

pub struct Database {
    signals: HashMap<String, Signal>,
    messages: HashMap<String, Message>,
    extra: DatabaseType,
}
