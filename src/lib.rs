//! `mboxShell` — a fast terminal MBOX viewer for files of any size.
//!
//! This crate provides the core library for parsing MBOX files, building
//! binary indexes, and accessing individual messages efficiently.

pub mod config;
pub mod error;
pub mod export;
pub mod i18n;
pub mod index;
pub mod model;
pub mod parser;
pub mod search;
pub mod store;
pub mod tui;
