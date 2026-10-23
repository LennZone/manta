---
title: Aurora
---

# Aurora

[![Build](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/example/aurora/actions) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Aurora** is a tiny, *fast* static site generator written in Rust. It turns a
folder of Markdown into a website in milliseconds — no ~~JavaScript~~ build
step, no config. Read the [guide](docs/guide.md) or jump to [Usage](#usage).

![The Aurora dashboard](images/screenshot.png "Aurora in dark mode")

## Features

- [x] Instant rebuilds with file watching
- [x] Syntax highlighting for 40+ languages
- [ ] Image optimisation
- [ ] Plugin API
  - Lua scripting
  - WebAssembly modules

> [!NOTE]
> Aurora requires Rust 1.80 or newer.

## Usage

Install it with `cargo` and point it at a folder:

```bash
cargo install aurora-ssg
aurora build ./site --out ./public
```

A minimal plugin looks like this:

```rust
use aurora::{Page, Plugin};

/// Adds reading time to every page.
pub struct ReadingTime;

impl Plugin for ReadingTime {
    fn on_page(&self, page: &mut Page) {
        let words = page.body.split_whitespace().count();
        page.meta.insert("minutes", (words / 200).max(1).to_string());
    }
}
```

### Options

| Flag | Default | Description |
|:-----|:-------:|------------:|
| `--out` | `./public` | Output folder |
| `--watch` | off | Rebuild when files change |
| `--drafts` | off | Include pages marked as draft |

## Why another generator?

> Simplicity is prerequisite for reliability.
>
> — Edsger W. Dijkstra

1. Zero configuration by default
2. One binary, no runtime
3. Readable output[^1]

---

Made with care in Berlin. See [CHANGELOG](CHANGELOG.md#v020) for what's new.

[^1]: Every page is plain HTML you can read and diff.
