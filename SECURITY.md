# Security Policy

## Reporting a vulnerability

Please **do not report security vulnerabilities in public issues, discussions or pull
requests.** Report them privately instead, in one of these ways:

- **GitHub:** [report a vulnerability](https://github.com/web-workers-com/manta/security/advisories/new)
  (private vulnerability reporting), or
- **Email:** [sven@manta-filemanager.com](mailto:sven@manta-filemanager.com)

Please include:

- the Manta version (Manta → About Manta) and the macOS version,
- a description of the vulnerability and its impact,
- steps to reproduce it, or a proof of concept,
- whether the issue is already known to others or public.

You will get an acknowledgement as soon as possible, usually within a few days. We will keep
you informed while we investigate and fix the issue, agree on a disclosure date with you, and
credit you in the release notes and the advisory unless you prefer to stay anonymous. Please
give us a reasonable amount of time to release a fix before you disclose the issue publicly.

## Supported versions

Only the **latest release** receives security fixes. Manta updates itself (Manta → Check for
Updates…), and the latest version is always available from
[manta-filemanager.com](https://manta-filemanager.com/download/Manta.dmg) and
[GitHub Releases](https://github.com/web-workers-com/manta/releases).

| Version | Supported |
|---|---|
| Latest release | Yes |
| Older releases | No |

## Scope

In scope are Manta itself (the app and its built-in plugins), the update mechanism and the
release artefacts. Some behaviour is by design and documented:

- **External plugins run in a macOS sandbox** (`sandbox-exec`) that enforces their declared
  permissions (`read_files`, `write_files`, `network`, `run_programs`, `terminal`); a way out
  of that sandbox, or a request to Manta that bypasses a missing permission, is a
  vulnerability. With the permissions they declare they can do what those allow. Where
  `sandbox-exec` doesn't work they run unsandboxed with a visible warning. Manta starts them
  switched off and asks before the first start; only install plugins you trust. See
  [docs/plugins.md](docs/plugins.md#security).
- **Quick Actions** run the shell commands you configure yourself.
- **Server credentials** (passwords, key passphrases, S3 secret keys) are stored in the macOS
  Keychain, not in Manta's settings files. SFTP host keys are checked against
  `~/.ssh/known_hosts`; a changed host key is refused.

## How updates are protected

Manta updates itself from a static feed over HTTPS. Every update is verified before it is
installed; the full scheme and threat model are in [docs/updates.md](docs/updates.md).

- **Signed feed.** The feed `update.json` comes with a detached Ed25519 signature
  (`update.json.sig`) over its exact bytes. Manta verifies it with the public key compiled
  into the app before it parses anything.
- **Signed releases.** Each release entry carries its own Ed25519 signature over the app ID,
  version, channel, minimum macOS version, and the size and SHA-256 hash of the update ZIP.
  A signature can't be reused for another version, app or channel.
- **Verified download.** The ZIP's size and SHA-256 hash must match the signed values.
- **Verified bundle.** The unpacked app must have the expected bundle identifier and exactly
  the promised version, and `codesign --verify --deep --strict` must pass. If the running
  app is signed with a Developer ID, the update must be signed by the same team (Team ID).
- **No downgrades and no plain HTTP.** Only versions newer than the running one are offered;
  `http://` feeds are refused (except `localhost` for tests).
- **Atomic install.** The old app is swapped for the new one atomically and restored if
  anything fails.

The private signing key lives only on the publisher's machine. It is never stored in this
repository (`.gitignore` blocks `*signing-key*`), and only the public key
(`packaging/update-public-key.txt`) is committed.

Not protected (see the threat model in [docs/updates.md](docs/updates.md#threat-model)): a
stolen signing key, a compromised build machine, and someone who controls the web space
withholding new updates by serving an older, validly signed feed.
