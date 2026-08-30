use std::fmt;

pub struct LoadError {
    pub path: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to load {}", self.path)
    }
}
