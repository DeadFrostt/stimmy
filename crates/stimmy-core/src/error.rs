//! Errors safe to show to a person.
//!
//! Configured add-on URLs routinely carry credentials in their path or query,
//! and `reqwest::Error`'s `Display` prints the URL. Nothing in this module keeps
//! the underlying error text for network failures, so a message can't leak one.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The request never completed: DNS, connection, or timeout.
    Offline,
    /// The server answered with a failure status.
    Server(u16),
    /// The server answered, but not with what the protocol promises.
    Unreadable,
    /// The URL a person entered can't be used as an add-on.
    InvalidAddon(&'static str),
    /// No installed add-on offers this resource.
    Unsupported,
    /// Artwork was refused: not an image, or over the size limit.
    BadArtwork,
    /// Stremio's own explanation, such as a wrong password. Safe to show as is.
    Account(String),
    /// A local file couldn't be read or written.
    Storage(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Offline => f.write_str("Can’t reach the server. Check your connection."),
            Self::Server(status) if *status >= 500 => {
                write!(
                    f,
                    "The server is having trouble ({status}). Try again shortly."
                )
            }
            Self::Server(404) => f.write_str("The server doesn’t have this."),
            Self::Server(status) => write!(f, "The server refused the request ({status})."),
            Self::Unreadable => f.write_str("The server sent something Stimmy can’t read."),
            Self::InvalidAddon(reason) => f.write_str(reason),
            Self::Unsupported => f.write_str("None of your add-ons offer this."),
            Self::Account(message) => f.write_str(message),
            Self::BadArtwork => f.write_str("This artwork couldn’t be used."),
            Self::Storage(reason) => write!(f, "Couldn’t save to disk: {reason}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        if let Some(status) = error.status() {
            Self::Server(status.as_u16())
        } else if error.is_decode() {
            Self::Unreadable
        } else {
            Self::Offline
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
