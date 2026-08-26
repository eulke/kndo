//! Bridges `SmolStr` into rkyv's archive model — `smol_str` has no native rkyv support (only
//! `serde`/`borsh`), so every `SmolStr` field in the graph vocabulary that participates in the
//! `graph.bin` snapshot (`cache.rs`) carries a `#[rkyv(with = SmolStrAsString)]` (or,
//! for `Option<SmolStr>`, `#[rkyv(with = rkyv::with::Map<SmolStrAsString>)]`) annotation
//! pointing here. This is the only place that knows the bridge exists; archived form is a plain
//! `rkyv::string::ArchivedString` — nothing about the *live* types' in-memory representation
//! changes, and nothing outside this module needs to know `SmolStr` isn't natively archivable.

// kndo:allow-file untested these impls are invoked from rkyv-derive-generated code (the
// #[rkyv(with = …)] expansions); dispatch through generated code is invisible to source
// extraction, while every snapshot round-trip test exercises them — internal/detection-gaps.md §2.

use crate::adapter::TypeExpr;
use rkyv::rancor::{Fallible, Source};
use rkyv::ser::{Allocator, Writer};
use rkyv::string::{ArchivedString, StringResolver};
use rkyv::vec::{ArchivedVec, VecResolver};
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::Place;
use smol_str::SmolStr;

pub(crate) struct SmolStrAsString;

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

/// Bridges [`TypeExpr`] — a recursive tree — into rkyv, which cannot derive `Archive` for a
/// type that recurses through `Vec<Self>` (the trait solver overflows on
/// `Vec<TypeExpr>: Archive`). The archived form is a **preorder walk with arity**: each atom
/// carries its own arity, and its children are exactly the next `arity` subtrees, so a cursor
/// rebuilds the tree with no indices to dangle and no separate node table to keep consistent.
///
/// This is the module's whole reason for existing, applied to a second type: the archive
/// format does not get to dictate the contract's shape. A type expression IS a tree, adapters
/// write it as one, and nothing outside this file knows it is stored flat.
pub(crate) struct TypeExprAsFlat;

/// One node of the preorder encoding. `tag` discriminates the [`TypeExpr`] variant; the
/// fields not meaningful for a tag are simply zero.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
// kndo:allow internal-only its archived form is the associated `Archived` type of a pub(crate) trait impl, so Rust requires it to be at least that visible — the narrowing this advises is not expressible
pub(crate) struct FlatAtom {
    /// 0 = `Named`, 1 = `Param`, 2 = `Unknown`. An unknown tag rebuilds as `Unknown`, so a
    /// snapshot written by a future version degrades to silence rather than to a wrong type.
    tag: u8,
    name: String,
    param: u32,
    arity: u32,
}

fn flatten(expr: &TypeExpr, out: &mut Vec<FlatAtom>) {
    match expr {
        TypeExpr::Named { name, args } => {
            out.push(FlatAtom {
                tag: 0,
                name: name.to_string(),
                param: 0,
                arity: args.len() as u32,
            });
            for arg in args {
                flatten(arg, out);
            }
        }
        TypeExpr::Param(n) => out.push(FlatAtom {
            tag: 1,
            name: String::new(),
            param: *n as u32,
            arity: 0,
        }),
        TypeExpr::Unknown => out.push(FlatAtom {
            tag: 2,
            name: String::new(),
            param: 0,
            arity: 0,
        }),
    }
}

fn rebuild(atoms: &ArchivedVec<ArchivedFlatAtom>, cursor: &mut usize) -> TypeExpr {
    let Some(atom) = atoms.get(*cursor) else {
        return TypeExpr::Unknown; // truncated input: silence, never a fabricated type
    };
    *cursor += 1;
    match atom.tag {
        0 => {
            let arity = atom.arity.to_native() as usize;
            let mut args = Vec::with_capacity(arity);
            for _ in 0..arity {
                args.push(rebuild(atoms, cursor));
            }
            TypeExpr::Named {
                name: SmolStr::new(atom.name.as_str()),
                args,
            }
        }
        1 => TypeExpr::Param(atom.param.to_native() as usize),
        _ => TypeExpr::Unknown,
    }
}

impl ArchiveWith<TypeExpr> for TypeExprAsFlat {
    type Archived = ArchivedVec<ArchivedFlatAtom>;
    type Resolver = VecResolver;

    fn resolve_with(field: &TypeExpr, resolver: Self::Resolver, out: Place<Self::Archived>) {
        let mut atoms = Vec::new();
        flatten(field, &mut atoms);
        ArchivedVec::resolve_from_len(atoms.len(), resolver, out);
    }
}

impl<S> SerializeWith<TypeExpr, S> for TypeExprAsFlat
where
    S: Fallible + Writer + Allocator + ?Sized,
    S::Error: Source,
{
    fn serialize_with(field: &TypeExpr, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        let mut atoms = Vec::new();
        flatten(field, &mut atoms);
        ArchivedVec::serialize_from_slice(&atoms, serializer)
    }
}

impl<D> DeserializeWith<ArchivedVec<ArchivedFlatAtom>, TypeExpr, D> for TypeExprAsFlat
where
    D: Fallible + ?Sized,
{
    fn deserialize_with(
        field: &ArchivedVec<ArchivedFlatAtom>,
        _: &mut D,
    ) -> Result<TypeExpr, D::Error> {
        Ok(rebuild(field, &mut 0))
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

    #[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, PartialEq)]
    struct TreeProbe {
        #[rkyv(with = TypeExprAsFlat)]
        yields: TypeExpr,
    }

    #[test]
    fn a_nested_type_expression_survives_the_archive() {
        // The shape the whole bridge exists for: `Result<Vec<TreeEntry>, _>`, three levels
        // deep, with the argument that a flat one-level encoding used to throw away.
        let value = TreeProbe {
            yields: TypeExpr::Named {
                name: SmolStr::new("Result"),
                args: vec![
                    TypeExpr::Named {
                        name: SmolStr::new("Vec"),
                        args: vec![TypeExpr::Named {
                            name: SmolStr::new("TreeEntry"),
                            args: Vec::new(),
                        }],
                    },
                    TypeExpr::Unknown,
                ],
            },
        };
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&value).unwrap();
        let back: TreeProbe = rkyv::from_bytes::<TreeProbe, rkyv::rancor::Error>(&bytes).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn a_param_reference_survives_the_archive() {
        let value = TreeProbe {
            yields: TypeExpr::Named {
                name: SmolStr::new("Result"),
                args: vec![TypeExpr::Param(0), TypeExpr::Unknown],
            },
        };
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&value).unwrap();
        let back: TreeProbe = rkyv::from_bytes::<TreeProbe, rkyv::rancor::Error>(&bytes).unwrap();
        assert_eq!(back, value);
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
