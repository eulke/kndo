//! Bridges `SmolStr` into rkyv's archive model — `smol_str` has no native rkyv support (only
//! `serde`/`borsh`), so every `SmolStr` field in the graph vocabulary that participates in the
//! `graph.bin` snapshot (`cache.rs`, ADR 0004) carries a `#[rkyv(with = SmolStrAsString)]` (or,
//! for `Option<SmolStr>`, `#[rkyv(with = rkyv::with::Map<SmolStrAsString>)]`) annotation
//! pointing here. This is the only place that knows the bridge exists; archived form is a plain
//! `rkyv::string::ArchivedString` — nothing about the *live* types' in-memory representation
//! changes, and nothing outside this module needs to know `SmolStr` isn't natively archivable.

use rkyv::rancor::{Fallible, Source};
use rkyv::ser::{Allocator, Writer};
use rkyv::string::{ArchivedString, StringResolver};
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::Place;
use smol_str::SmolStr;

pub struct SmolStrAsString;

impl ArchiveWith<SmolStr> for SmolStrAsString {
    type Archived = ArchivedString;
    type Resolver = StringResolver;

    fn resolve_with(field: &SmolStr, resolver: Self::Resolver, out: Place<Self::Archived>) {
        ArchivedString::resolve_from_str(field.as_str(), resolver, out);
    }
}

impl<S> SerializeWith<SmolStr, S> for SmolStrAsString
where
    S: Fallible + Writer + Allocator + ?Sized,
    S::Error: Source,
{
    fn serialize_with(field: &SmolStr, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        ArchivedString::serialize_from_str(field.as_str(), serializer)
    }
}

impl<D> DeserializeWith<ArchivedString, SmolStr, D> for SmolStrAsString
where
    D: Fallible + ?Sized,
{
    fn deserialize_with(field: &ArchivedString, _: &mut D) -> Result<SmolStr, D::Error> {
        Ok(SmolStr::new(field.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, PartialEq)]
    struct Probe {
        #[rkyv(with = SmolStrAsString)]
        name: SmolStr,
        #[rkyv(with = rkyv::with::Map<SmolStrAsString>)]
        maybe: Option<SmolStr>,
    }

    #[test]
    fn round_trips_through_the_archived_form() {
        let value = Probe {
            name: SmolStr::new("hello"),
            maybe: Some(SmolStr::new("world")),
        };
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&value).unwrap();
        let archived = rkyv::access::<ArchivedProbe, rkyv::rancor::Error>(&bytes).unwrap();
        assert_eq!(archived.name.as_str(), "hello");
        let back: Probe = rkyv::deserialize::<Probe, rkyv::rancor::Error>(archived).unwrap();
        assert_eq!(back, value);
    }
}
