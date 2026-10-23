# manta-plugin-protocol

Message types and a tiny stdin/stdout loop for writing
[Manta](https://manta-filemanager.com) plugins in Rust. MIT licensed.

```toml
[dependencies]
manta-plugin-protocol = { git = "https://github.com/web-workers-com/manta" }
```

```rust
use manta_plugin_protocol::{run, HostMessage, Out, PluginMessage, ToastLevel};

fn main() -> std::io::Result<()> {
    run(|msg, out: &mut Out| {
        if let HostMessage::RunCommand { id, .. } = msg {
            if id == "hello" {
                out.send(&PluginMessage::toast(ToastLevel::Success, "Hello from Rust!"))?;
            }
        }
        Ok(())
    })
}
```

- The crate's major version is the protocol's `api_version` (1).
- No dependency on Manta's internal crates: only `serde` and `serde_json`.
- Manta's test suite (`crates/plugin/tests/sdk_compat.rs`) checks these types
  against the host's own, so they can't drift apart.

A complete template is in
[`examples/plugins/hello-rust`](../../examples/plugins/hello-rust); the
protocol reference is [`docs/plugins.md`](../../docs/plugins.md).
