use crate::config::Config;

struct Builder {
    config: Config,
}

pub(crate) fn build() -> Result<usize, u8> {
    let b = Builder { config: Config };
    b.finish()
}

impl Builder {
    fn finish(&self) -> Result<usize, u8> {
        let chir = self.config.build_many(1)?;
        Ok(chir.line_terminator())
    }
}
