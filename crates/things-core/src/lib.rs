//! Core model for Things: entities are folders, components are files.
//!
//! An entity has no type or schema. It is a folder named with a readable
//! slug plus a short ID (`mass-effect-7f3a`), kept in a [`collection`]
//! folder such as `games/` so different kinds of things stay apart. Every file in
//! that folder is a component, and each component owns its file format.
//! This crate only knows about entities and raw component files; parsing a
//! component into something useful is the component's job (see
//! [`components`] for the data formats shipped so far).

pub mod collection;
pub mod components;
mod id;
pub mod radar;
pub mod research;
pub mod search;
mod store;
pub mod tasks;

pub use collection::{Collection, Manifest};
pub use id::{EntityId, ParseIdError};
pub use store::{ComponentFile, Entity, Store, StoreError};
