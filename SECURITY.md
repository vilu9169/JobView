# Security notes

JobView is a local Windows application. SQLite contains readable email and job data. Windows Credential Manager protects OAuth setup, refresh credentials, and optional Gemini API keys; access tokens stay in backend memory. There is no plaintext credential fallback or cloud application server. Gmail is read-only. Explicitly enabled Gemini sends minimized email text to Google; see [AI privacy and setup](docs/gemini-setup.md) and [architecture](docs/architecture.md).

Do not include credentials, auth callback URLs, tokens, databases, or personal mail in public issues or pull requests. Use the repository's private security reporting feature if its owner enables it. Use redacted descriptions in public discussion and request a private channel for sensitive details.

If credentials are exposed, revoke the affected grant in Google Account connections, rotate Gemini API keys in AI Studio, and replace the affected client setup through Google Cloud as appropriate. Removing a file from the latest Git tree does not remove earlier committed copies.

This is an early desktop release. Gmail has deterministic automated coverage; live-account consent and clean-machine distribution testing remain separate [release checks](CONTRIBUTING.md#release-checks).
