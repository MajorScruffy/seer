use std::fmt;

/// Usage errors exit 2; all other variants exit 3.
#[derive(Debug, PartialEq, Eq)]
pub enum SeerError {
    Usage(String),
    Io(String),
}

impl SeerError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Io(_) => 3,
        }
    }
}

impl fmt::Display for SeerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(msg) | Self::Io(msg) => f.write_str(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_is_exit_2() {
        assert_eq!(SeerError::Usage("bad args".into()).exit_code(), 2);
    }

    #[test]
    fn runtime_is_exit_3() {
        assert_eq!(SeerError::Io("read failed".into()).exit_code(), 3);
    }
}
