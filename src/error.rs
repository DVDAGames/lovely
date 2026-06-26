use std::fmt::{self, Display};
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum LovelyError {
    Io {
        path: Option<PathBuf>,
        source: io::Error,
    },
    Config(String),
    Lock(String),
    Archive(String),
    Command(String),
}

impl LovelyError {
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: Some(path.into()),
            source,
        }
    }

    pub fn plain_io(source: io::Error) -> Self {
        Self::Io { path: None, source }
    }
}

impl Display for LovelyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LovelyError::Io {
                path: Some(path),
                source,
            } => {
                write!(f, "{}: {}", path.display(), source)
            }
            LovelyError::Io { path: None, source } => write!(f, "{source}"),
            LovelyError::Config(message) => write!(f, "configuration error: {message}"),
            LovelyError::Lock(message) => write!(f, "lockfile error: {message}"),
            LovelyError::Archive(message) => write!(f, "archive error: {message}"),
            LovelyError::Command(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for LovelyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LovelyError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, LovelyError>;
