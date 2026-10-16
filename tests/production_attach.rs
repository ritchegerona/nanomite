//! Production wiring checks.
//!
//! Integration tests compile the library *without* `cfg(test)`, so this is the
//! only place the real `App::new` attach path is exercised: memory must come up
//! against the default locations, honour the `NANOMITE_MEMORY_*` policy
//! environment, and stay silent about failures.
//!
//! Both locations are redirected into a private temp directory first, so the
//! developer's real `~/.config/nanomite/memory` tree is never opened.

use nanomite::app::App;
use nanomite::config::Config;

/// One test function on purpose: these phases mutate process-wide environment
/// variables, and separate `#[test]`s in one binary would race on them.
#[tokio::test]
async fn memory_attaches_at_the_configured_location_and_honours_the_master_switch() {
    let base = std::env::temp_dir().join(format!("nanomite-attach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("temp base");

    std::env::set_var("NANOMITE_CONFIG", base.join("config.toml"));
    std::env::set_var("NANOMITE_MEMORY_DIR", base.join("memory"));
    std::env::set_var("NANOMITE_MEMORY_DB", base.join("data").join("nanomite.db"));
    std::env::remove_var("NANOMITE_MEMORY");

    let cfg = Config::from_env();
    let app = App::new(cfg, tokio::runtime::Handle::current()).expect("app");

    // Attached: the six memory tools are live and no hint is shown.
    assert!(app.memory_attached(), "memory must attach by default");
    assert!(app.memory_hint().is_none(), "no hint when attached");
    assert!(
        base.join("memory").is_dir(),
        "the configured memory root was created"
    );

    // Policy: NANOMITE_MEMORY=0 is the documented master switch.
    std::env::set_var("NANOMITE_MEMORY", "0");
    let cfg = Config::from_env();
    let off = App::new(cfg, tokio::runtime::Handle::current()).expect("app");
    assert!(
        !off.memory_attached(),
        "NANOMITE_MEMORY=0 must disable memory"
    );
    let hint = off.memory_hint().expect("a quiet hint");
    assert!(hint.contains("NANOMITE_MEMORY=1"), "{hint}");
    std::env::remove_var("NANOMITE_MEMORY");

    let _ = std::fs::remove_dir_all(&base);
}
