//! Engine domain types, domain and application services, and the ports they use.
//!
//! The crate is platform-neutral. It makes no OS or IO calls and depends only on
//! foundation, model and contract crates, never on an adapter or app. Service-part
//! crates are its peers: neither depends on the other. It defines each port trait its
//! services consume; engine adapters under `native/` implement those ports, and
//! `mixrelay-service` composes them into the engine executable. Block processing and
//! rendering belong to `mixrelay-audio-engine`, and engine process lifetime and
//! ownership belong to `mixrelay-engine-process`.
