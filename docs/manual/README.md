# Manta user manual

The user manual in seven languages: German (`de/`, primary), English (`en/`), Simplified Chinese (`zh-Hans/`), Spanish (`es/`), French (`fr/`), Italian (`it/`) and Russian (`ru/`) – the same languages as the app UI. The help window (⌘?) shows the manual in the app language and falls back to English. It is written for people who use Manta, not for developers; developer docs live one level up in `docs/`.

## Layout

```
docs/manual/
├── index.json          ordered table of contents, titles, search keywords
├── de/NN-slug.md       German chapters
├── en/NN-slug.md       English chapters (same file names and anchors)
├── zh-Hans/, es/, fr/, it/, ru/   further translations (same file names and anchors)
├── i18n/LANG.json      chapter titles and keywords for zh-Hans, es, fr, it, ru
└── images/
    ├── NEEDED.md       list of screenshots to generate
    ├── de/NAME.png     German screenshots (NAME-dark.png for dark variants)
    ├── en/NAME.png     English screenshots
    └── zh-Hans/, es/, fr/, it/, ru/   the same screenshots per language
```

All languages use the **same file names** (the German slug) and the **same heading anchors**, so a link such as `05-dateien.md#loeschen` works in every language. A chapter in one language is always a translation of the same chapter in the others.

## index.json

```json
{
  "format": 1,
  "languages": ["de", "en", "zh-Hans", "es", "fr", "it", "ru"],
  "fallback_language": "en",
  "path": "{lang}/{file}",
  "chapters": [
    {
      "id": "dateien",
      "number": 5,
      "file": "05-dateien.md",
      "title": { "de": "Dateien verwalten", "en": "Managing Files" },
      "keywords": { "de": ["kopieren", "…"], "en": ["copy", "…"] }
    }
  ]
}
```

- `chapters` is in reading order. `id` equals the chapter's top-level heading anchor.
- `path` tells the app how to find a chapter: `de/05-dateien.md`.
- `keywords` are extra search terms (synonyms, English terms in German search, technical names) that don't necessarily appear in the text. Search should match them in addition to the full text.
- `fallback_language`: if the UI language has no manual, use this one.

## Markdown conventions

The chapters are plain GitHub-flavoured Markdown, rendered in the help window with the Markdown Preview plugin's renderer (`crates/plugin-markdown`). They use only what that renderer supports:

- **Heading anchors**: every heading that is linked to has an explicit id, `## Delete {#loeschen}` (pulldown-cmark heading attributes, enabled in the renderer). Headings without an id get the renderer's automatic slug. Ids are ASCII, German, lowercase, and identical in all languages.
- **Links between chapters**: relative, `[Delete](05-dateien.md#loeschen)`; within a chapter `[…](#anchor)`. The help window should open these in the help window, not in the file pane.
- **Links to developer docs**: `../../plugins.md`, `../../quick-actions.md`, `../../project-info.md`. They resolve in the repository. Inside the app they point outside the manual: open them in the browser (GitHub) or leave them inactive.
- **Callouts**: GitHub alerts, `> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`, `> [!CAUTION]`.
- **Images**: `![Alt text](images/NAME.png)`. The help window resolves `images/` to `images/<lang>/` (and `NAME-dark.png` in dark mode, if present), falling back to English (`fallback_language`), then to any other language. The alt text describes the picture for when it is missing.
- **Shortcuts** are written with glyphs in the order ⌃⌥⇧⌘ + key (`⇧⌘P`), as in the app.
- **UI labels** are bold and written exactly as in the app's string catalog (`crates/app/src/i18n/catalog/*.rs`, the plugins' `i18n.rs`).
- **Review markers**: `<!-- VERIFY: … -->` marks a statement to check against the app (features still being built, or behaviour that wasn't verified in code). HTML comments are not rendered. Remove a marker once checked.

## Full-text search

Suggested approach for the help window:

1. Load every chapter of the current language, split at `##` headings into sections (heading text + anchor + body text, Markdown stripped, HTML comments removed).
2. Match the query (case- and accent-insensitive) against section headings, body text and the chapter's `keywords`.
3. Rank heading matches above body matches; show chapter title › section heading and a snippet; open the chapter at the section anchor.

## Style

- German: du-form, Finder terminology (Ablage, Objekt, Bereich, Schnellansicht, „In den Papierkorb legen“), German quotes „…“, a space before the ellipsis in labels (`Umbenennen …`).
- English: plain US spelling as in the app's English catalog ("Favorites", but "colour" where the app says "Accent colour").
- Each chapter starts with a one- or two-sentence summary. Task sections are numbered steps.
- Honest about limitations: permanent delete on servers, no Keychain for passwords, the FTP clear-data fallback, the macOS 26 folder handler, Remove Download for third-party clouds.
