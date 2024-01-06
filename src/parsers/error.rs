#[derive(Debug)]
pub enum Error {
    IO(String),
    NotImplemented,
}

impl From<std::io::Error> for Error {
    fn from(item: std::io::Error) -> Self {
        Error::IO(item.to_string())
    }
}
