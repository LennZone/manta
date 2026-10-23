//! The standalone SDK crate `manta-plugin-protocol` (for third-party Rust
//! plugins) and the JSON examples in `docs/plugins.md` must agree with the
//! host's own protocol types. Every message type goes host → SDK → host (or
//! SDK → host → SDK) and must come back unchanged.

use std::collections::BTreeSet;
use std::path::PathBuf;

use manta_plugin::protocol::{self as host, Parsed};
use manta_plugin_protocol as sdk;

/// One example per host → plugin message type.
const HOST_SAMPLES: &[&str] = &[
    r#"{"type":"init","api_version":1,"plugin_id":"hello","data_dir":"/Users/anna/Library/Application Support/Manta/plugin-data/hello","panes":[{"pane":"left","path":"/Users/anna","local_path":"/Users/anna","protocol":"Local","backend_label":"Local","selection":["/Users/anna/a.txt"],"active":true},{"pane":2,"path":"/srv","local_path":null,"protocol":"Docker","backend_label":"web (Docker)","selection":[],"active":false,"remote":{"protocol":"Docker","user":"","host":"web","port":0}}],"locale":"zh-Hans"}"#,
    r#"{"type":"locale_changed","locale":"de"}"#,
    r#"{"type":"directory_changed","pane_state":{"pane":"right","path":"/var/www","local_path":null,"protocol":"Sftp","backend_label":"deploy@web-01 (SFTP)","selection":[],"active":false,"remote":{"protocol":"Sftp","user":"deploy","host":"web-01","port":22}}}"#,
    r#"{"type":"listing_refreshed","pane_state":{"pane":"left","path":"/a","local_path":"/a","protocol":"Local","backend_label":"Local","selection":[],"active":true}}"#,
    r#"{"type":"selection_changed","pane_state":{"pane":3,"path":"/a","local_path":"/a","protocol":"Local","backend_label":"Local","selection":["/a/b"],"active":true}}"#,
    r#"{"type":"active_pane_changed","pane":"right"}"#,
    r#"{"type":"pane_opened","pane":2}"#,
    r#"{"type":"pane_closed","pane":2}"#,
    r#"{"type":"file_action","action":{"id":3,"pane":"left","kind":"copy","sources":["/Users/anna/a.txt"],"destination":"/var/www","src_local":true,"dst_local":false,"src_label":"Local","dst_label":"deploy@web-01 (SFTP)","phase":{"finished":{"ok":true,"files":1,"bytes":5120,"errors":[],"cancelled":false,"elapsed_ms":420}},"src_remote":null,"dst_remote":{"protocol":"Sftp","user":"deploy","host":"web-01","port":22},"sync":null}}"#,
    r#"{"type":"run_command","id":"hello","pane":{"pane":"left","path":"/Users/anna","local_path":"/Users/anna","protocol":"Local","backend_label":"Local","selection":[],"active":true}}"#,
    r#"{"type":"panel_action","panel":"info","action":"refresh","pane":{"pane":"left","path":"/Users/anna","local_path":"/Users/anna","protocol":"Local","backend_label":"Local","selection":[],"active":true}}"#,
    r#"{"type":"shutdown"}"#,
    r#"{"type":"rename_result","pane":"left","dir":"/p","ok":[["a","b"]],"failed":[["c","exists"]]}"#,
    r#"{"type":"dir_listing","pane":"left","dir":"/p","entries":[{"name":"IMG.JPG","is_dir":false,"size":2000000,"modified":1760950800,"shown":true}],"error":null}"#,
    r#"{"type":"file_content","pane":"right","path":"/var/www/README.md","data":"IyBXZWxjb21lCg==","size":10,"truncated":false,"modified":1760950800,"error":null}"#,
];

/// One example per plugin → host message type (all optional fields set
/// where the type has them).
const PLUGIN_SAMPLES: &[&str] = &[
    r#"{"type":"set_decorations","pane":"left","dir":"/p","entries":{"big.iso":{"badge":{"text":"L","tone":"warning"},"tooltip":"Large"},"target":{"dimmed":true,"name_tone":"muted"}}}"#,
    r#"{"type":"set_sizes","pane":2,"dir":"/p","sizes":{"src":{"bytes":184320,"complete":true,"files":42,"on_disk":200704,"unreadable":0}}}"#,
    r#"{"type":"set_chips","pane":"left","dir":"/p","chips":[{"text":"main","icon":"branch","tone":"accent","tooltip":"2 ahead","command":"status"}]}"#,
    r#"{"type":"toast","level":"success","text":"Renamed 12 items","button":"Undo","command":"undo"}"#,
    r#"{"type":"navigate","pane":"right","path":"/etc/nginx","remote":{"protocol":"Docker","user":"","host":"web","port":0}}"#,
    r#"{"type":"register_commands","commands":[{"id":"hello","title":"Say Hello","shortcut":"⌃⌥H","context":"file_context_menu"}]}"#,
    r#"{"type":"register_panels","panels":[{"id":"wizard","title":"Wizard","placement":"modal","default_open":false,"size":[640.0,480.0]}]}"#,
    r#"{"type":"set_panel_content","panel":"info","pane":"left","blocks":[{"type":"heading","text":"report.pdf"},{"type":"text","text":"Not backed up","tone":"warning"},{"type":"code","text":"$ make"},{"type":"key_value","rows":[{"key":"Size","value":"1.8 MB","tone":"neutral"}]},{"type":"list","rows":[{"icon":"file","text":"a.txt","detail":"2 KB","tone":"neutral","action":"open:a.txt","tooltip":"Open"}]},{"type":"buttons","buttons":[{"label":"Refresh","action":"refresh","primary":true}]},{"type":"separator"},{"type":"section","text":"Scripts","detail":"package.json"},{"type":"tags","tags":[{"text":"pnpm","tone":"success","icon":"package"}]},{"type":"image","path":"/tmp/x.png","max_height":240.0,"caption":"1920×1080"}]}"#,
    r#"{"type":"set_panel_open","panel":"info","pane":"left","open":true}"#,
    r#"{"type":"toggle_panel","panel":"project","pane":"right"}"#,
    r#"{"type":"rename_many","pane":"left","dir":"/p","renames":[["IMG_0041.JPG","Iceland-41.jpg"]]}"#,
    r#"{"type":"list_dir","pane":"left","dir":"/p"}"#,
    r#"{"type":"read_file","pane":"right","path":"/var/www/README.md","max_bytes":2097152}"#,
    r#"{"type":"run_in_terminal","pane":"left","command":"ls -la","cwd":"/p"}"#,
    r#"{"type":"refresh_pane","pane":"left"}"#,
    r#"{"type":"select","pane":"left","paths":["/p/a","/p/b"]}"#,
    r#"{"type":"set_status_items","items":[{"id":"cpu","text":"CPU 91 %","compact":"","tooltip":"10 cores","icon":"cpu","level":"warn","meter":0.91,"priority":30,"action":{"kind":"open_app","bundle_id":"com.apple.ActivityMonitor"}}]}"#,
    r#"{"type":"log","level":"warn","message":"scanned 120 files"}"#,
];

fn ty(line: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(line).unwrap();
    v["type"].as_str().unwrap().to_string()
}

#[test]
fn type_lists_match() {
    assert_eq!(sdk::PLUGIN_MESSAGE_TYPES, host::PLUGIN_MESSAGE_TYPES);
    assert_eq!(sdk::API_VERSION, host::PROTOCOL_VERSION);
    let host_types: BTreeSet<String> = HOST_SAMPLES.iter().map(|l| ty(l)).collect();
    let sdk_types: BTreeSet<String> = sdk::HOST_MESSAGE_TYPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        host_types, sdk_types,
        "every host message type has a sample and an SDK variant"
    );
    let plugin_types: BTreeSet<String> = PLUGIN_SAMPLES.iter().map(|l| ty(l)).collect();
    let all: BTreeSet<String> = host::PLUGIN_MESSAGE_TYPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(plugin_types, all, "every plugin message type has a sample");
}

#[test]
fn host_messages_survive_the_sdk() {
    for line in HOST_SAMPLES {
        let h: host::HostMessage =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("host: {e}\n{line}"));
        let wire = h.to_line();
        let s = sdk::HostMessage::parse(&wire).unwrap_or_else(|e| panic!("sdk: {e}\n{wire}"));
        assert_ne!(s, sdk::HostMessage::Unknown, "{wire}");
        let back: host::HostMessage =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, h, "{line}");
    }
}

#[test]
fn plugin_messages_survive_the_host() {
    for line in PLUGIN_SAMPLES {
        let s: sdk::PluginMessage =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("sdk: {e}\n{line}"));
        let wire = s.to_line();
        let h = match host::parse_line(&wire) {
            Parsed::Message(m) => m,
            other => panic!("host rejected the SDK's {wire}: {other:?}"),
        };
        let back: sdk::PluginMessage = serde_json::from_value(serde_json::to_value(&*h).unwrap())
            .unwrap_or_else(|e| {
                panic!(
                    "sdk can't read the host's {}: {e}",
                    serde_json::to_string(&*h).unwrap()
                )
            });
        assert_eq!(back, s, "{line}");
    }
}

/// Every JSON object with a `"type"` in docs/plugins.md is a valid message
/// in one direction or the other.
#[test]
fn docs_examples_parse() {
    let doc = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins.md");
    let text = std::fs::read_to_string(doc).unwrap();
    let mut checked = 0;
    for block in text.split("```json").skip(1) {
        let block = block.split("```").next().unwrap();
        let candidates: Vec<String> = match serde_json::from_str::<serde_json::Value>(block) {
            Ok(_) => vec![block.to_string()],
            Err(_) => block
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(str::to_string)
                .collect(),
        };
        for c in candidates {
            let v: serde_json::Value = serde_json::from_str(&c)
                .unwrap_or_else(|e| panic!("bad JSON in docs/plugins.md: {e}\n{c}"));
            let Some(t) = v.get("type").and_then(|t| t.as_str()) else {
                continue;
            };
            let compact = serde_json::to_string(&v).unwrap();
            if host::PLUGIN_MESSAGE_TYPES.contains(&t) {
                assert!(
                    matches!(host::parse_line(&compact), Parsed::Message(_)),
                    "docs example rejected by the host: {compact}"
                );
                serde_json::from_str::<sdk::PluginMessage>(&compact)
                    .unwrap_or_else(|e| panic!("SDK: {e}\n{compact}"));
            } else {
                serde_json::from_str::<host::HostMessage>(&compact)
                    .unwrap_or_else(|e| panic!("docs example: {e}\n{compact}"));
                assert_ne!(
                    sdk::HostMessage::parse(&compact).unwrap(),
                    sdk::HostMessage::Unknown,
                    "{compact}"
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 30, "only {checked} examples found");
}
