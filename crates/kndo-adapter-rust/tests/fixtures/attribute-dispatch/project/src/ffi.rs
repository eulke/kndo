#[unsafe(no_mangle)]
pub extern "C" fn exported_symbol() {}

pub fn touch() {}

#[allow(dead_code)]
fn kept_on_purpose() {}

#[expect(unused)]
fn expected_unused() {}

fn truly_dead() {}

#[cfg(test)]
mod tests {
    #[test]
    fn touches() {
        super::touch();
    }
}
