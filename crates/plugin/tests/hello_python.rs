//! The Hello Python example (examples/plugins/hello-python): its Python
//! unit tests, including the English/German tables and an end-to-end
//! `locale_changed` check. Skipped when `python3` is missing.

use std::path::PathBuf;

#[test]
fn python_unit_tests_pass() {
    let ok = std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("python3 not found, skipping");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/hello-python");
    let out = std::process::Command::new("python3")
        .args(["-B", "-m", "unittest", "discover", "-s", "tests", "-v"])
        .current_dir(dir)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "python unit tests failed:\n{text}");
    assert!(text.contains("OK"), "{text}");
}
