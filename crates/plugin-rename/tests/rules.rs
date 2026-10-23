//! Every rule type, the pipeline order, the preview's conflicts and diff,
//! and presets.

use chrono::{NaiveDate, NaiveDateTime};
use manta_plugin_rename::presets::{load_last, save_last, Presets};
use manta_plugin_rename::preview::{compute, diff, Issue, Item};
use manta_plugin_rename::rules::*;

fn today() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, 20)
        .unwrap()
        .and_hms_opt(9, 30, 5)
        .unwrap()
}

fn ctx(index: usize) -> ItemCtx {
    ItemCtx {
        index,
        is_dir: false,
        modified: NaiveDate::from_ymd_opt(2024, 3, 7)
            .unwrap()
            .and_hms_opt(14, 5, 0),
        today: today(),
    }
}

fn run(rules: &[Rule], name: &str) -> String {
    compile(rules).apply(name, &ctx(0))
}

fn rule(kind: RuleKind) -> Rule {
    Rule {
        enabled: true,
        kind,
    }
}

fn replace(find: &str, rep: &str, regex: bool, case: bool, all: bool, target: Target) -> Rule {
    rule(RuleKind::Replace {
        find: find.into(),
        replace: rep.into(),
        regex,
        case_sensitive: case,
        all,
        target,
    })
}

#[test]
fn splits_names() {
    let s = |n: &str, d: bool| {
        let p = NameParts::split(n, d);
        (p.dot, p.stem, p.ext)
    };
    assert_eq!(
        s("photo.JPG", false),
        (false, "photo".into(), Some("JPG".into()))
    );
    assert_eq!(
        s("archive.tar.gz", false),
        (false, "archive.tar".into(), Some("gz".into()))
    );
    assert_eq!(s("README", false), (false, "README".into(), None));
    assert_eq!(s(".bashrc", false), (true, "bashrc".into(), None));
    assert_eq!(
        s(".config.json", false),
        (true, "config".into(), Some("json".into()))
    );
    assert_eq!(s("trailing.", false), (false, "trailing.".into(), None));
    assert_eq!(s("my.folder", true), (false, "my.folder".into(), None));
    assert_eq!(s(".", false), (false, ".".into(), None));
    for n in ["photo.JPG", ".bashrc", "a.b.c", "x."] {
        assert_eq!(NameParts::split(n, false).join(), n);
    }
}

#[test]
fn find_and_replace_text() {
    let r = replace("IMG", "Photo", false, false, true, Target::Name);
    assert_eq!(
        run(std::slice::from_ref(&r), "IMG_001.img"),
        "Photo_001.img",
        "extension untouched"
    );
    assert_eq!(
        run(&[r], "img_img.jpg"),
        "Photo_Photo.jpg",
        "case-insensitive by default, all"
    );
    let first = replace("a", "x", false, true, false, Target::Name);
    assert_eq!(run(&[first], "banana.txt"), "bxnana.txt");
    let case = replace("A", "x", false, true, true, Target::Name);
    assert_eq!(run(&[case], "Aaa.txt"), "xaa.txt");
    // Special regex characters are literal in text mode, `$1` too.
    let lit = replace("(1)", "$1", false, false, true, Target::Name);
    assert_eq!(run(&[lit], "file (1).txt"), "file $1.txt");
    // Extension only / both.
    let ext = replace("jpeg", "jpg", false, false, true, Target::Extension);
    assert_eq!(run(&[ext], "jpeg.jpeg"), "jpeg.jpg");
    let both = replace(".", "_", false, false, true, Target::Both);
    assert_eq!(run(&[both], "a.b.txt"), "a_b_txt");
}

#[test]
fn regex_with_groups_and_errors() {
    let r = replace(
        r"(\d{4})-(\d{2})-(\d{2})",
        "$3.$2.$1",
        true,
        false,
        true,
        Target::Name,
    );
    assert_eq!(run(&[r], "Scan 2024-03-07.pdf"), "Scan 07.03.2024.pdf");
    // `$1x` means group 1 followed by "x".
    let r = replace(r"^(\w+)", "$1x", true, false, false, Target::Name);
    assert_eq!(run(&[r], "abc def.txt"), "abcx def.txt");
    // `$$` is a literal dollar.
    let r = replace("price", "$$5", true, false, true, Target::Name);
    assert_eq!(run(&[r], "price.txt"), "$5.txt");
    // A broken regex is reported and skipped; other rules still run.
    let bad = replace("(unclosed", "x", true, false, true, Target::Name);
    let up = rule(RuleKind::Case {
        mode: CaseMode::Upper,
        target: Target::Name,
    });
    let rules = [bad, up];
    let c = compile(&rules);
    assert_eq!(c.errors.len(), 1);
    assert_eq!(c.errors[0].0, 0);
    assert!(
        !c.errors[0].1.is_empty() && !c.errors[0].1.contains('\n'),
        "{}",
        c.errors[0].1
    );
    assert_eq!(c.apply("abc.txt", &ctx(0)), "ABC.txt");
    // A disabled broken rule is no error.
    let mut off = rules.clone();
    off[0].enabled = false;
    assert!(compile(&off).errors.is_empty());
}

#[test]
fn unicode_in_replace_and_positions() {
    let r = replace("ü", "ue", false, false, true, Target::Name);
    assert_eq!(
        run(&[r], "Grüße Ü.txt"),
        "Grueße ue.txt",
        "case-insensitive matches Ü too"
    );
    let ins = rule(RuleKind::Insert {
        text: "→".into(),
        at: InsertAt::Position,
        position: 2,
        target: Target::Name,
    });
    assert_eq!(
        run(&[ins], "äöü.txt"),
        "äö→ü.txt",
        "positions count characters, not bytes"
    );
    let rm = rule(RuleKind::Remove {
        mode: RemoveMode::First,
        count: 2,
        from: 0,
        target: Target::Name,
    });
    assert_eq!(run(&[rm], "日本語.txt"), "語.txt");
}

#[test]
fn insert_text() {
    let mk = |at, position, target| {
        rule(RuleKind::Insert {
            text: "X".into(),
            at,
            position,
            target,
        })
    };
    assert_eq!(
        run(&[mk(InsertAt::Start, 0, Target::Name)], "a.txt"),
        "Xa.txt"
    );
    assert_eq!(
        run(&[mk(InsertAt::End, 0, Target::Name)], "a.txt"),
        "aX.txt"
    );
    assert_eq!(
        run(&[mk(InsertAt::Position, 1, Target::Name)], "abc.txt"),
        "aXbc.txt"
    );
    assert_eq!(
        run(&[mk(InsertAt::Position, 99, Target::Name)], "abc.txt"),
        "abcX.txt",
        "clamped"
    );
    assert_eq!(
        run(&[mk(InsertAt::End, 0, Target::Extension)], "a.txt"),
        "a.txtX"
    );
    assert_eq!(
        run(&[mk(InsertAt::End, 0, Target::Extension)], "noext"),
        "noext",
        "no extension: nothing"
    );
    assert_eq!(
        run(&[mk(InsertAt::End, 0, Target::Both)], "a.txt"),
        "a.txtX"
    );
    // Dotfiles stay hidden.
    assert_eq!(
        run(&[mk(InsertAt::Start, 0, Target::Name)], ".env"),
        ".Xenv"
    );
}

#[test]
fn remove_characters() {
    let mk = |mode, count, from| {
        rule(RuleKind::Remove {
            mode,
            count,
            from,
            target: Target::Name,
        })
    };
    assert_eq!(
        run(&[mk(RemoveMode::First, 4, 0)], "IMG_1234.jpg"),
        "1234.jpg"
    );
    assert_eq!(
        run(&[mk(RemoveMode::Last, 2, 0)], "photo_v2.jpg"),
        "photo_.jpg"
    );
    assert_eq!(run(&[mk(RemoveMode::Range, 3, 1)], "abcdef.txt"), "aef.txt");
    assert_eq!(
        run(&[mk(RemoveMode::Range, 3, 10)], "abc.txt"),
        "abc.txt",
        "out of range: nothing"
    );
    assert_eq!(
        run(&[mk(RemoveMode::First, 10, 0)], "abc.txt"),
        ".txt",
        "can empty the stem"
    );
    let ext = rule(RuleKind::Remove {
        mode: RemoveMode::Last,
        count: 1,
        from: 0,
        target: Target::Extension,
    });
    assert_eq!(run(&[ext], "a.jpeg"), "a.jpe");
}

#[test]
fn change_case_modes() {
    let mk = |mode, target| rule(RuleKind::Case { mode, target });
    assert_eq!(
        run(&[mk(CaseMode::Lower, Target::Name)], "HeLLo World.TXT"),
        "hello world.TXT"
    );
    assert_eq!(
        run(&[mk(CaseMode::Upper, Target::Both)], "straße.txt"),
        "STRASSE.TXT"
    );
    assert_eq!(
        run(
            &[mk(CaseMode::Title, Target::Name)],
            "the QUICK brown-fox_jumps.txt"
        ),
        "The Quick Brown-Fox_Jumps.txt"
    );
    assert_eq!(
        run(
            &[mk(CaseMode::Title, Target::Name)],
            "2024 report (final).txt"
        ),
        "2024 Report (Final).txt"
    );
    assert_eq!(
        run(&[mk(CaseMode::Sentence, Target::Name)], "hELLO wORLD.txt"),
        "Hello world.txt"
    );
    assert_eq!(
        run(
            &[mk(CaseMode::Sentence, Target::Name)],
            "  ünïcode test.txt"
        ),
        "  Ünïcode test.txt"
    );
    assert_eq!(
        run(&[mk(CaseMode::Upper, Target::Name)], ".gitignore"),
        ".GITIGNORE"
    );
}

#[test]
fn numbering() {
    let mk = |start, step, pad, place, sep: &str| {
        rule(RuleKind::Number {
            start,
            step,
            pad,
            place,
            sep: sep.into(),
        })
    };
    let rules = [mk(1, 1, 3, Place::Prefix, " - ")];
    let c = compile(&rules);
    assert_eq!(c.apply("a.jpg", &ctx(0)), "001 - a.jpg");
    assert_eq!(c.apply("b.jpg", &ctx(41)), "042 - b.jpg");
    let rules = [mk(10, 5, 0, Place::Suffix, "_")];
    assert_eq!(compile(&rules).apply("a.jpg", &ctx(2)), "a_20.jpg");
    let rules = [mk(1, 1, 2, Place::Replace, "")];
    assert_eq!(compile(&rules).apply("whatever.png", &ctx(8)), "09.png");
    let rules = [mk(0, -1, 2, Place::Prefix, "")];
    assert_eq!(
        compile(&rules).apply("a", &ctx(3)),
        "-03a",
        "negative numbers keep the sign"
    );
}

#[test]
fn dates() {
    let mk = |source, format: &str, place| {
        rule(RuleKind::Date {
            source,
            format: format.into(),
            place,
            sep: "_".into(),
        })
    };
    assert_eq!(
        run(
            &[mk(DateSource::Modified, "YYYY-MM-DD", Place::Prefix)],
            "scan.pdf"
        ),
        "2024-03-07_scan.pdf"
    );
    assert_eq!(
        run(
            &[mk(DateSource::Today, "YYYYMMDD", Place::Suffix)],
            "scan.pdf"
        ),
        "scan_20261020.pdf"
    );
    assert_eq!(
        run(
            &[mk(DateSource::Today, "DD.MM.YY hh-mm-ss", Place::Prefix)],
            "a"
        ),
        "20.10.26 09-30-05_a"
    );
    // Unknown modification date: unchanged.
    let rules = [mk(DateSource::Modified, "YYYY", Place::Prefix)];
    let mut c0 = ctx(0);
    c0.modified = None;
    assert_eq!(compile(&rules).apply("a.txt", &c0), "a.txt");
    for f in DATE_PRESETS {
        assert!(!format_date(&today(), f).contains('Y'), "{f}");
    }
}

#[test]
fn extensions() {
    let mk = |mode, new_ext: &str| {
        rule(RuleKind::Extension {
            mode,
            new_ext: new_ext.into(),
        })
    };
    assert_eq!(run(&[mk(ExtMode::Lower, "")], "IMG.JPG"), "IMG.jpg");
    assert_eq!(run(&[mk(ExtMode::Upper, "")], "a.pdf"), "a.PDF");
    assert_eq!(
        run(&[mk(ExtMode::Change, ".jpeg")], "a.jpg"),
        "a.jpeg",
        "leading dot ignored"
    );
    assert_eq!(
        run(&[mk(ExtMode::Change, "md")], "README"),
        "README.md",
        "adds one where there was none"
    );
    assert_eq!(run(&[mk(ExtMode::Change, "")], "a.txt"), "a");
    assert_eq!(run(&[mk(ExtMode::Remove, "")], "a.tar.gz"), "a.tar");
    assert_eq!(run(&[mk(ExtMode::Keep, "")], "a.TXT"), "a.TXT");
    assert_eq!(
        run(&[mk(ExtMode::Remove, "")], ".bashrc"),
        ".bashrc",
        "a dotfile has no extension"
    );
    // Folders have no extension.
    let rules = [mk(ExtMode::Upper, "")];
    let mut d = ctx(0);
    d.is_dir = true;
    assert_eq!(compile(&rules).apply("my.folder", &d), "my.folder");
}

#[test]
fn clean_up_and_umlauts() {
    let mk = |trim, collapse, spaces, umlauts| {
        rule(RuleKind::Clean {
            trim,
            collapse,
            spaces,
            umlauts,
            target: Target::Name,
        })
    };
    assert_eq!(
        run(
            &[mk(true, true, SpaceMode::Keep, false)],
            "  my   holiday  .jpg"
        ),
        "my holiday.jpg"
    );
    assert_eq!(
        run(
            &[mk(true, true, SpaceMode::Dash, false)],
            "my  holiday photo.jpg"
        ),
        "my-holiday-photo.jpg"
    );
    assert_eq!(
        run(
            &[mk(false, false, SpaceMode::Underscore, false)],
            "a b  c.jpg"
        ),
        "a_b__c.jpg"
    );
    assert_eq!(
        run(
            &[mk(true, true, SpaceMode::Keep, true)],
            "Grüße aus Köln.txt"
        ),
        "Gruesse aus Koeln.txt"
    );
    assert_eq!(transliterate("ÄRGER Äpfel"), "AERGER Aepfel");
    assert_eq!(transliterate("Café crème naïve"), "Cafe creme naive");
    // Decomposed (NFD) input as macOS may store it.
    assert_eq!(transliterate("Mu\u{308}ller Cafe\u{301}"), "Mueller Cafe");
}

#[test]
fn pipeline_runs_in_order_and_skips_disabled_rules() {
    let rep = replace("a", "b", false, true, true, Target::Name);
    let rep2 = replace("b", "c", false, true, true, Target::Name);
    assert_eq!(run(&[rep.clone(), rep2.clone()], "a.txt"), "c.txt");
    assert_eq!(
        run(&[rep2.clone(), rep.clone()], "a.txt"),
        "b.txt",
        "order matters"
    );
    let mut off = rep2.clone();
    off.enabled = false;
    assert_eq!(run(&[rep.clone(), off], "a.txt"), "b.txt");
    // Numbering after upper-case keeps the number; before it, too.
    let num = rule(RuleKind::Number {
        start: 1,
        step: 1,
        pad: 2,
        place: Place::Prefix,
        sep: "-".into(),
    });
    let up = rule(RuleKind::Case {
        mode: CaseMode::Upper,
        target: Target::Both,
    });
    assert_eq!(run(&[num, up], "a.jpg"), "01-A.JPG");
    // Rules survive a JSON round trip (presets).
    let rules = vec![rep, rep2];
    let json = serde_json::to_string(&rules).unwrap();
    let back: Vec<Rule> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, rules);
    // Missing optional fields use defaults.
    let r: Rule = serde_json::from_str(r#"{"type":"replace","find":"x"}"#).unwrap();
    assert!(r.enabled);
    assert!(matches!(r.kind, RuleKind::Replace { all: true, .. }));
}

fn items(names: &[&str]) -> Vec<Item> {
    names
        .iter()
        .map(|n| Item {
            name: n.to_string(),
            is_dir: false,
            modified: None,
        })
        .collect()
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn conflicts_are_detected() {
    let all = names(&["a.txt", "b.txt", "keep.txt", "KEEP2.txt"]);
    // Duplicate targets.
    let r = [rule(RuleKind::Number {
        start: 1,
        step: 0,
        pad: 0,
        place: Place::Replace,
        sep: String::new(),
    })];
    let p = compute(&items(&["a.txt", "b.txt"]), &all, &r, today(), false);
    assert_eq!(p.conflicts, 2);
    assert!(p.rows.iter().all(|r| r.issue == Some(Issue::Duplicate)));
    assert!(!p.can_apply());
    // Existing (untouched) file.
    let r = [replace("a", "keep", false, true, true, Target::Name)];
    let p = compute(&items(&["a.txt", "b.txt"]), &all, &r, today(), false);
    assert_eq!(p.rows[0].issue, Some(Issue::Exists));
    assert_eq!(p.rows[1].issue, None);
    assert!(!p.rows[1].changed, "b is unchanged");
    // Case-insensitive volume: "keep2" collides with "KEEP2".
    let r = [replace("a", "keep2", false, true, true, Target::Name)];
    assert_eq!(
        compute(&items(&["a.txt"]), &all, &r, today(), true).rows[0].issue,
        Some(Issue::Exists)
    );
    assert_eq!(
        compute(&items(&["a.txt"]), &all, &r, today(), false).rows[0].issue,
        None
    );
    // Empty name, invalid characters, too long.
    let r = [rule(RuleKind::Remove {
        mode: RemoveMode::First,
        count: 99,
        from: 0,
        target: Target::Both,
    })];
    assert_eq!(
        compute(&items(&["a.txt"]), &all, &r, today(), false).rows[0].issue,
        Some(Issue::Empty)
    );
    let r = [replace("a", "x/y", false, true, true, Target::Name)];
    assert_eq!(
        compute(&items(&["a.txt"]), &all, &r, today(), false).rows[0].issue,
        Some(Issue::InvalidChar('/'))
    );
    let r = [replace("a", "x:y", false, true, true, Target::Name)];
    assert_eq!(
        compute(&items(&["a.txt"]), &all, &r, today(), false).rows[0].issue,
        Some(Issue::InvalidChar(':'))
    );
    let long = "x".repeat(300);
    let r = [replace("a", &long, false, true, true, Target::Name)];
    assert_eq!(
        compute(&items(&["a.txt"]), &all, &r, today(), false).rows[0].issue,
        Some(Issue::TooLong)
    );
    // A swap is fine: both names are freed.
    let r = [
        replace("^a", "TMP", true, true, true, Target::Name),
        replace("^b", "a", true, true, true, Target::Name),
        replace("TMP", "b", false, true, true, Target::Name),
    ];
    let p = compute(&items(&["a.txt", "b.txt"]), &all, &r, today(), false);
    assert_eq!(p.rows[0].new, "b.txt");
    assert_eq!(p.rows[1].new, "a.txt");
    assert_eq!(p.conflicts, 0, "{:?}", p.rows);
    assert!(p.can_apply());
    assert_eq!(p.renames().len(), 2);
    // Case-only rename on a case-insensitive volume is no conflict.
    let r = [rule(RuleKind::Case {
        mode: CaseMode::Upper,
        target: Target::Name,
    })];
    let p = compute(&items(&["a.txt"]), &all, &r, today(), true);
    assert_eq!(p.rows[0].new, "A.txt");
    assert_eq!(p.conflicts, 0);
    // A bad regex blocks applying.
    let r = [replace("(", "x", true, true, true, Target::Name)];
    let p = compute(&items(&["a.txt"]), &all, &r, today(), false);
    assert_eq!(p.rule_errors.len(), 1);
    assert!(!p.can_apply());
}

#[test]
fn character_diff() {
    let (o, n) = diff("IMG_0042.JPG", "Holiday 0042.jpg");
    let removed: String = o
        .iter()
        .filter(|s| s.changed)
        .map(|s| s.text.as_str())
        .collect();
    let added: String = n
        .iter()
        .filter(|s| s.changed)
        .map(|s| s.text.as_str())
        .collect();
    let kept: String = n
        .iter()
        .filter(|s| !s.changed)
        .map(|s| s.text.as_str())
        .collect();
    assert!(kept.contains("0042."), "{n:?}");
    assert_eq!(
        o.iter().map(|s| s.text.as_str()).collect::<String>(),
        "IMG_0042.JPG"
    );
    assert_eq!(
        n.iter().map(|s| s.text.as_str()).collect::<String>(),
        "Holiday 0042.jpg"
    );
    assert!(removed.contains("JPG") && added.contains("jpg"));
    let (o, n) = diff("same", "same");
    assert!(o.iter().chain(&n).all(|s| !s.changed));
    let (_, n) = diff("ab", "aXb");
    assert_eq!(n.len(), 3);
    assert!(n[1].changed && n[1].text == "X");
    let (o, n) = diff("äöü", "aöu");
    assert!(o[0].changed && !o[1].changed && n[0].changed, "{o:?} {n:?}");
}

#[test]
fn presets_save_load_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let mut p = Presets::load(tmp.path());
    assert!(p.items.is_empty());
    p.upsert("Photos", vec![RuleType::Number.new_rule()]);
    p.upsert(
        "archive",
        vec![RuleType::Date.new_rule(), RuleType::Case.new_rule()],
    );
    p.upsert("photos", vec![RuleType::Clean.new_rule()]);
    assert_eq!(p.items.len(), 2, "same name (ignoring case) replaces");
    assert_eq!(p.items[0].name, "archive", "sorted");
    p.save(tmp.path()).unwrap();
    let back = Presets::load(tmp.path());
    assert_eq!(back, p);
    let mut back = back;
    assert!(back.remove("archive"));
    assert!(!back.remove("nope"));
    back.save(tmp.path()).unwrap();
    assert_eq!(Presets::load(tmp.path()).items.len(), 1);
    // A broken file means no presets, not a crash.
    std::fs::write(tmp.path().join("presets.json"), "{oops").unwrap();
    assert!(Presets::load(tmp.path()).items.is_empty());
    // The last rules.
    assert!(load_last(tmp.path()).is_empty());
    save_last(tmp.path(), &[RuleType::Insert.new_rule()]).unwrap();
    assert_eq!(load_last(tmp.path()).len(), 1);
}
