# Contributing to Manta

Thank you for helping to improve Manta! Bug reports, translations, documentation, plugins and
code are all welcome. By taking part you agree to follow our
[Code of Conduct](CODE_OF_CONDUCT.md).

- **Bugs and ideas:** open an issue with the bug report or feature request form. Please check
  the [manual](https://manta-filemanager.com/manual/en/) and existing issues first.
- **Security vulnerabilities:** never in a public issue; see [SECURITY.md](SECURITY.md).
- **Larger changes:** please open an issue first, so we can agree on the approach before you
  spend time on it.

## Setup

You need a Mac with Apple silicon, macOS with the Xcode Command Line Tools
(`xcode-select --install`) and Rust via [rustup](https://rustup.rs/) with the `rustfmt` and
`clippy` components.

```sh
git clone https://github.com/web-workers-com/manta.git
cd manta
cargo run -p manta
```

[docs/development.md](docs/development.md) explains the crates, release builds, the tests and
where Manta keeps its data.

## Code style

- Format with rustfmt: `cargo fmt --all`. CI runs `cargo fmt --all --check`.
- Clippy must report zero warnings:

  ```sh
  cargo clippy --workspace --all-targets -- -D warnings
  ```

- Keep changes focused: one topic per pull request, no unrelated reformatting or renames.
- Follow the patterns of the surrounding code. The crates' `lib.rs` files document their
  public API contracts; don't change those signatures without discussing it first.
- Never put secrets (API keys, passwords, signing keys) into the code or the repository.
- New dependencies need a licence that is compatible with MIT and no copyleft licence.
  After changing dependencies, regenerate the licence notices with
  `scripts/third-party-licenses.py` and commit
  [docs/third-party-licenses.md](docs/third-party-licenses.md); the release build refuses to
  run when that file is stale.

## Tests

```sh
scripts/test.sh          # unit and UI tests; run this before every pull request
scripts/test.sh full     # also the integration tests (Docker, S3, SSH, Git, terminal)
```

Add or update tests for what you change. Bug fixes should come with a test that fails
without the fix. Integration tests must skip cleanly when their service is not available and
may only touch resources they create themselves (see
[docs/development.md](docs/development.md#tests)).

## Translations

Manta's UI is available in seven languages: English, German, Simplified Chinese, Spanish,
French, Italian and Russian. The rules:

- **Every UI string goes through a catalog**, never a string literal in the UI code. Source
  tables are `(key, English, German)` tuples in `crates/app/src/i18n/catalog/*.rs` (the app)
  and `crates/plugin-*/src/i18n.rs` (built-in plugins); each further language has its own
  table (`…/zh/`, `…/es/`, `…/fr/`, `…/it/`, `…/ru/`). The tests reject hard-coded UI strings.
- **New or changed strings need all seven languages.** After adding keys to a source table,
  run `python3 scripts/i18n/skeleton.py`: it adds the missing keys to every translation table with
  the English text and a `// TODO` marker. Translate those lines.
- **`python3 scripts/i18n/check.py` must report `0 problems`.** It finds missing and unknown keys,
  TODO markers, placeholder mismatches, untranslated copies of English, incomplete plural
  forms and punctuation that doesn't fit the language. `python3 scripts/i18n/check.py fr` checks one
  language, `--stats` shows the coverage.
- **Use Apple's macOS terminology** in every language, and keep placeholders such as
  `{count}` or `{name}` unchanged. Plurals use the CLDR forms of each language (`key.one`,
  `key.other`; Russian also `key.few` and `key.many`).
- **Glossary:** [docs/i18n/glossary.md](docs/i18n/glossary.md) is generated from the catalogs;
  don't edit it by hand. When you change short UI terms (menus, commands, buttons, settings
  labels), regenerate it:

  ```sh
  MANTA_UPDATE_GLOSSARY=1 cargo test -p manta glossary
  ```

Improvements to existing translations by native speakers are very welcome.

## Documentation

If you change behaviour that users see, update the user manual in
[docs/manual/](docs/manual/) in all seven languages (the chapters share file names and
heading anchors across languages; see [docs/manual/README.md](docs/manual/README.md)).
Quote UI terms exactly as the glossary lists them. Developer-facing changes belong in
`docs/` (for example [docs/plugins.md](docs/plugins.md) for the plugin protocol).

## Commit messages

- A short summary line (about 72 characters or less) that says what changed and where, for
  example `Compare & Sync: keep the selection after a refresh`.
- If needed, a blank line and a body that explains *why*.
- Reference issues where it helps (`Fixes #123`).

## Pull requests

1. Fork the repository and create a branch from the default branch.
2. Make your change, with tests and documentation.
3. Run the checks locally:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   scripts/test.sh
   python3 scripts/i18n/check.py
   ```

4. Open a pull request and fill in the template. Screenshots help for UI changes (light and
   dark, and a long language such as German or Russian if the layout is tight).
5. CI must pass. A maintainer reviews the change; please answer review comments with
   follow-up commits rather than force-pushing over them.

## Licence

Manta is licensed under the [MIT licence](LICENSE). By contributing, you agree that your
contributions are licensed under the same licence.
