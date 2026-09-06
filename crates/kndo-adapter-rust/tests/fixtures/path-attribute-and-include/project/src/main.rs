mod a;

include!("gen/tables.rs");

fn main() {
    a::go();
    println!("{}", TABLE);
}
