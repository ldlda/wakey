use std::{fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// Failure to resolve a host name to IPs.
    DnsResolve { name: String, source: io::Error },
    /// External command failed (e.g., ip neigh)
    CommandFailed {
        cmd: &'static str,
        args: Vec<String>,
        status: Option<i32>,
        stderr: String,
    },
    /// Generic IO error fallback
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::DnsResolve { name, source } => write!(f, "DNS resolve failed for {name}: {source}"),
            Error::CommandFailed { cmd, args, status, stderr } => {
                let code = status.map(|c| c.to_string()).unwrap_or_else(|| "signal".into());
                write!(
                    f,
                    "{cmd} {args:?} failed (status: {code}): {stderr}",
                )
            }
            Error::Io(e) => write!(f, "IO error: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::DnsResolve { source, .. } => Some(source),
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self { Error::Io(e) }
}