# Connect Gmail to JobView

JobView uses your own Google OAuth Desktop app client and requests only `https://www.googleapis.com/auth/gmail.readonly`. No Google project or client is included in the repository.

## 1. Create your Google project

Open [Google Cloud Console](https://console.cloud.google.com/), select or create a project named JobView, and enable the [Gmail API](https://console.cloud.google.com/apis/library/gmail.googleapis.com) in that project. The same link is available in Settings. [Google's Gmail setup reference](https://developers.google.com/workspace/gmail/api/quickstart/python) describes the Cloud prerequisites.

## 2. Configure consent

Open **Google Auth Platform → Branding** and complete the app name, support email, and contact email. For a personal Gmail account, choose **External**, keep the publishing status **Testing**, and add your Gmail address under **Audience → Test users**. In **Data Access**, add `https://www.googleapis.com/auth/gmail.readonly`. Use an internal audience only for a Workspace organization where that option applies. See [Google's consent configuration guide](https://developers.google.com/workspace/guides/configure-oauth-consent).

## 3. Download a Desktop app client

Under **Google Auth Platform → Clients**, choose **Create client → Desktop app**, name it JobView Desktop, and download the JSON. Save it outside the checkout. A Web application client, service account, API key, or manually edited JSON will not work. JobView chooses a temporary loopback port for each login; no manually configured web redirect URI is needed. See [Google's desktop OAuth documentation](https://developers.google.com/identity/protocols/oauth2/native-app).

## 4. Connect in JobView

1. Launch the desktop app and open **Settings**.
2. Choose **Import OAuth JSON** and select the downloaded file. Cancelling the picker leaves setup unchanged. The backend saves configuration in Windows Credential Manager; the UI never receives file contents.
3. Choose **Connect Gmail** and finish Google's sign-in/consent flow in your system browser. Select the account added as a test user. You can cancel from JobView; an unfinished sign-in times out after three minutes.
4. Return to JobView and confirm the connected address. Connecting alone does not download mail.
5. Save your sync limit (default **500**, allowed **1–5,000**) and choose **Sync now**. Review **Job Emails**, **Other Mail**, and **Unmatched**.

Keep the downloaded JSON private or remove it from Downloads after a successful import if you do not need that copy. Never upload it, your local database, or real message fixtures to GitHub or a bug report.

## Later syncs

Sync runs only when requested. After the initial recent-message import, JobView uses Gmail history to retrieve changes. It reuses cached bodies and updates labels locally. Deleted messages remain cached with a deletion notice, preserving job timelines. Increasing the initial limit also imports older recent messages; lowering it never deletes cached data.

Recent scans exclude Spam/Trash. Incremental discoveries also skip messages currently labeled Spam/Trash; restored messages can be imported later. Previously cached messages remain available if moved there, preserving source evidence. Current labels are checked after fetching a new message, so this is a cache policy rather than a guarantee that those message contents never reach the backend.

If history expires, JobView scans the configured recent range and reconciles metadata for previously cached mail without redownloading cached bodies. If a pass fails, successful messages stay cached and the checkpoint stays at the previous completed pass for a safe retry. Google's [synchronization guide](https://developers.google.com/workspace/gmail/api/guides/sync) documents history expiry and recovery.

One Gmail account is bound to a workspace. Disconnecting retains that binding and the cache; reconnect with the same account. Account switching and workspace reset are not yet supported UI workflows.

## Verify your first sync

Automated tests use fictional mail and simulated Google responses. Check these behaviors with your own client and account:

1. Confirm the connected address and read-only grant, then sync the default recent 500 messages. Inspect plain-text and HTML messages, dates, senders, and classifications.
2. Link an email, correct a stage or classification, and complete an action. Sync and restart JobView; confirm those changes persist.
3. Make a normal mailbox change in Gmail, then sync JobView to check label or deletion updates in the cache.
4. Disconnect and reconnect the same account; verify cached mail and jobs remain available. To test revoked-token recovery, revoke access through Google Account connections and reconnect.

## Troubleshooting

| Symptom                             | What to do                                                                                                                   |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| Access denied/account not allowed   | Check your exact Gmail address is a test user in the same Cloud project. Workspace administrators may need to allow the app. |
| Invalid client/import fails         | Download a fresh **Desktop app** JSON. Disconnect before replacing a connected configuration.                                |
| Gmail API disabled                  | Enable Gmail API in the project owning the imported client, then retry.                                                      |
| Login timeout/browser does not open | Cancel, retry with a working default browser, and allow the local loopback callback through local security software.         |
| Reconnect required                  | Connect again with the same account. The grant may have expired or been revoked.                                             |
| Network/rate-limit error            | Keep using the local tracker and retry Sync later. Successfully cached mail remains available.                               |
| Credential store unavailable        | Use Windows under an account with working Credential Manager access; there is no plaintext fallback.                         |

External apps in **Testing** receive refresh tokens expiring after seven days when requesting Gmail access. Expect periodic reconnection while testing. Distribution to other people involves Google's production/verification requirements. [Google documents refresh-token expiry](https://developers.google.com/identity/protocols/oauth2#expiration); see also [Gmail scope classifications](https://developers.google.com/workspace/gmail/api/auth/scopes).

**Disconnect** removes tokens from this device while retaining client setup and local data. It does not revoke Google's grant. To revoke it, remove JobView from [Google Account connections](https://myaccount.google.com/connections). JobView's disconnect and sync commands never change Gmail messages.
