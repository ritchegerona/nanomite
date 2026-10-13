// The scripted provider is a test double: it is compiled only for `cargo test`
// and never exists in the shipped binary. Production always talks to a real
// OpenAI-compatible endpoint.
#[cfg(test)]
pub mod testdouble;
pub mod openai;
pub mod provider;
pub mod runtime;
