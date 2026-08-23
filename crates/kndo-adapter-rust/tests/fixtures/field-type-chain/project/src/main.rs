mod hiargs;
mod lowargs;

fn main() {
    let low = crate::lowargs::LowArgs::parse();
    hiargs::finish(&low);
}
