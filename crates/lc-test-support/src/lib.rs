//! Shared test support for `lc-writer` and `lr2lc`: a stateful stand-in for a
//! running LightCraft app and a tiny fake control port.
//!
//! This crate is test-only (`publish = false`), is not part of the product, and
//! depends only on `serde_json` (already a workspace dependency). It builds on
//! Linux, macOS and Windows and adds no new external crate.
//!
//! See [`fake_lightcraft`] for the full fake server and [`fake_port`] for the
//! minimal request-recording port.

#![allow(dead_code)]

pub mod fake_lightcraft;
pub mod fake_port;
