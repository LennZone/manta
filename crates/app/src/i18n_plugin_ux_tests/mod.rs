//! German (and English) renders of plugin UI inside the app: one file per
//! plugin. Child of `ux_tests`, so it reuses the fixture and harness.
//! `i18n::set_locale` in a test also switches built-in plugins drawn on the
//! test thread and the `init.locale` of external plugins started from it.

mod disk_usage;
mod markdown;
mod project_info;
mod quick_actions;
mod rename;
mod system_load;
