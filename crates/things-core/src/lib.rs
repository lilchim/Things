//! Core model for Things: entities are folders, components are files.
//!
//! An entity has no type or schema. It is a folder under `entities/` named
//! with a readable slug plus a short ID (`mass-effect-7f3a`). Every file in
//! that folder is a component, and each component owns its file format.
//! This crate only knows about entities and raw component files; parsing a
//! component into something useful is the component's job (see
//! [`components`] for the data formats shipped so far).

pub mod components;
mod id;
pub mod search;
mod store;

pub use id::{EntityId, ParseIdError};
pub use store::{ComponentFile, Entity, Store, StoreError};
