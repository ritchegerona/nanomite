//! Authentication and authorization module.

pub mod jwt;
pub mod rbac;
pub mod password;

pub fn init() -> anyhow::Result<()> {
    Ok(())
}