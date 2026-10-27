// The scripted provider is a test double: it is compiled only for `cargo test`
// and never exists in the shipped binary. Production always talks to a real
// OpenAI-compatible endpoint.
pub mod openai;
pub mod policy;
pub mod provider;
pub mod runtime;
pub mod suggest;
pub mod swarm;
#[cfg(test)]
pub mod testdouble;
