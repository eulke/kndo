use std::fmt::Write;

pub trait Flag {
    fn doc_short(&self) -> &'static str;
    fn update(&self) -> bool;
}

pub struct AfterContext;

impl Flag for AfterContext {
    fn doc_short(&self) -> &'static str {
        "after context"
    }
    fn update(&self) -> bool {
        true
    }
}

pub const FLAGS: &[&dyn Flag] = &[&AfterContext];

pub fn render(flag: &dyn Flag) -> String {
    let mut out = String::new();
    write!(out, "{} {}", flag.doc_short(), flag.update()).unwrap();
    out
}
