//! A type alias is an item like any other: `type_item` puts its name in the
//! grammar's `name` field, the extractor declares it there, and one that
//! nobody names is dead. The associated `type` inside an `impl` is a MEMBER
//! and travels with its impl, which is why the same grammar kind reads two
//! ways and the ledger's row is about the item.

pub type Live = u8;

pub type Forgotten = u16;

pub struct Holder;

impl Iterator for Holder {
    type Item = Live;

    fn next(&mut self) -> Option<Live> {
        None
    }
}
