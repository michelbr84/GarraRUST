//! Skill Retriever — placeholder for [GAR-646].
//!
//! Concrete implementation of the [`crate::BeforeAction`] trait. Will match a
//! [`crate::SkillRequestContext`] against the registry via embedding similarity
//! (consuming `garraia-embeddings` from Fase 2.1) plus scope filter plus score
//! threshold.
//!
//! Until this lands, the runtime uses [`crate::NoopBeforeAction`] which always
//! returns `None`.
//!
//! [GAR-646]: https://linear.app/chatgpt25/issue/GAR-646
