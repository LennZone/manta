## What and why

<!-- What does this change, and why? Link the issue it fixes, e.g. "Fixes #123". -->

## How to test

<!-- Steps for a reviewer. For UI changes, add screenshots (light and dark). -->

## Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` reports no warnings
- [ ] `scripts/test.sh` passes (and `scripts/test.sh full` if backends, transfers or the terminal changed)
- [ ] Tests added or updated for the change
- [ ] New or changed UI strings are in the catalogs in all 7 languages (en, de, zh-Hans, es, fr, it, ru) and `python3 scripts/i18n/check.py` reports 0 problems
- [ ] The glossary is regenerated if short UI terms changed (`MANTA_UPDATE_GLOSSARY=1 cargo test -p manta glossary`)
- [ ] The user manual (`docs/manual/`, all 7 languages) and developer docs are updated, or no update is needed
- [ ] `docs/third-party-licenses.md` is regenerated if dependencies changed (`scripts/third-party-licenses.py`)
- [ ] I agree that my contribution is licensed under the MIT licence
