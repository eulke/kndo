const TEMPLATE_LONG: &str = "help {v}";
pub(super) fn generate_long() -> String {
    TEMPLATE_LONG.replace("{v}", "1.0")
}
