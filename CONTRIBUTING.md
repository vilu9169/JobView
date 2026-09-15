# Contributing

Follow [README](README.md) for Windows prerequisites and checks. Commit both lockfiles. Use fictional `.example` addresses and organizations in fixtures/tests; never add a mailbox export, database, OAuth download, token, or personal message body.

Markdown files are ignored unless explicitly listed in `.gitignore`. Keep internal plans and verification notes local; update the allowlist when adding maintained project documentation.

Rust owns persistence, integrations, credentials, and business rules. Keep React focused on presentation, use typed IPC, and expose narrow backend operations. Add migrations for schema changes; never rewrite a shipped migration.

Preserve manual decisions and timeline provenance. Sync failures must retain successful cache writes without advancing the checkpoint. Never hold a database transaction across a network request. Gmail stays read-only; Gemini defaults to Off and requires explicit opt-in before sending email text.

Describe changed behavior, meaningful validation, and migration/release impact in pull requests. Integration tests must not require personal credentials or live networks. See [architecture](docs/architecture.md) for the service boundaries and data model.

## Native credential-store test

The optional Windows Credential Manager test is ignored by default. It writes, reads, and removes a uniquely named fictional credential:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --locked windows_credential_manager_round_trip -- --ignored
```

## Release checks

1. Run the checks in [README](README.md#development-and-checks). Review staged files with `git diff --cached --stat` and `git diff --cached --check`; keep credentials, local data, build output, and internal working notes out of commits. After pushing, confirm **Actions → Windows checks** passes. CI requires no Gmail or Gemini credentials.
2. Complete the [Gmail acceptance checks](docs/gmail-setup.md#verify-your-first-sync) with your own OAuth client and account. If releasing Gemini changes, run the fictional connection test and inspect representative classifications using the [Gemini setup guide](docs/gemini-setup.md).
3. Run `npm.cmd run desktop:build` to build the configured NSIS installer; packaging may download additional tools. Test installation and launch on a clean Windows machine with WebView2, and verify existing data survives an upgrade.
4. Keep the [MIT license](LICENSE) with distributions. Publisher identity, release signing, automatic updates, and Google's production OAuth requirements need to be addressed before broader distribution.

The standalone executable build (`npm.cmd run tauri build -- --no-bundle`) does not verify installation, signing, or live-account behavior. Signed distribution and clean-machine acceptance remain outstanding.
