pub(crate) struct Config;

impl Config {
    pub(crate) fn build_many(&self, _n: usize) -> Result<ConfiguredHir, u8> {
        Ok(ConfiguredHir)
    }
}

pub(crate) struct ConfiguredHir;

impl ConfiguredHir {
    pub(crate) fn line_terminator(&self) -> usize {
        0
    }
}
