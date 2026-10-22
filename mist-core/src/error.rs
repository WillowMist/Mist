use thiserror::Error;

#[derive(Debug, Error)]
pub enum PuebloError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("telnet protocol violation: {0}")]
    Telnet(String),

    #[error("MCCP decompression failed: {0}")]
    Mccp(String),

    #[error("profile error: {0}")]
    Profile(String),

    #[error("TinTin error: {0}")]
    TinTin(String),

    #[error("markup parse error: {0}")]
    Markup(String),
}

pub type Result<T> = std::result::Result<T, PuebloError>;