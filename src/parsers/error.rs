#[derive(Debug)]
pub enum Error {
    IO(String),
    ExpectedComment,
    ExpectedToken,
    IncorrectToken,
    NumberParse,
    NotImplemented,
}

impl From<std::io::Error> for Error {
    fn from(item: std::io::Error) -> Self {
        Error::IO(item.to_string())
    }
}

impl From<std::num::ParseFloatError> for Error {
    fn from(_: std::num::ParseFloatError) -> Self {
        Error::NumberParse
    }
}
