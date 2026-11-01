//! Engine domain types, domain and application services, and the ports they use.
//!
//! The crate is platform-neutral. It makes no OS or IO calls and depends only on
//! foundation, model and contract crates and `mixrelay-time`, never on an adapter or
//! app. It defines each port trait its services consume; engine adapters under
//! `native/` implement those ports, and `mixrelay-service` composes them into the
//! engine executable. Block processing and rendering belong to `mixrelay-audio-engine`,
//! and engine process lifetime and ownership belong to `mixrelay-engine-process`.
