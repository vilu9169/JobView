# Optional Gemini classification

JobView can use Gemini to classify Swedish and English job-search emails. The tracker and local rules work without Gemini, and **AI defaults to Off**. Gmail authentication and the Gemini API key are separate; your Gmail OAuth configuration does not need to change.

Paste the complete key, including any punctuation. JobView accepts opaque keys up to Windows Credential Manager's 2,560-byte binary limit. Saving a key does not establish API access; use the connection test after saving.

## Setup

1. Open [Google AI Studio API keys](https://aistudio.google.com/api-keys), sign in, and create a key in your own Google Cloud project. You can select an existing project or create one. Follow [Google's key setup instructions](https://ai.google.dev/gemini-api/docs/api-key) if your project is not listed.
2. Enable billing on that project for paid API access. Before sending private email, review [Google's data-use terms](https://ai.google.dev/gemini-api/terms), including retention and regional requirements. Google's [pricing page](https://ai.google.dev/gemini-api/docs/pricing) distinguishes free-tier and paid-tier data use.
3. Open JobView and go to **Settings → Optional AI · Gemini**. Paste the key into the masked field and choose **Save API key**. The input clears immediately; Rust stores the key in Windows Credential Manager. Do not paste it into chat, source code, `.env`, or GitHub.
4. Start with the default **Gemini 2.5 Flash-Lite** and **50 maximum requests per run**. Choose **Test Gemini connection**. The test sends one fictional Swedish email and validates its structured response; it does not read your mailbox.
5. Choose **Review job candidates**, then **Save settings**. This mode lets Gemini reconsider high-confidence local classifications as well as less obvious candidates. **Uncertain emails only** costs less, but retains high-confidence local decisions.
6. Choose **Review cached emails**. Inspect the results in Job Emails, Other Mail, Unmatched, and Action Required. If the request cap is reached, run another normal review to continue. Later successful Gmail syncs run the enabled AI mode automatically, within the same per-run limit.

Use **Off → Save settings** to stop email transfer. **Cancel AI review** interrupts a review; a request already sent may still be billed. **Remove API key** removes that credential and switches AI off while preserving cached mail, results, and Gmail credentials. JobView does not revoke the key in Google's account; revoke it in AI Studio when required.

## Cost and model choice

The app supports `gemini-2.5-flash-lite` (default) and `gemini-3.1-flash-lite`. Check [Google's current pricing](https://ai.google.dev/gemini-api/docs/pricing) and model availability before enabling a review. Cost depends on the chosen model, text length, and provider usage, including prompt instructions and the response schema. No live accuracy comparison on representative mail has been completed; inspect decisions before deciding which model suits your workflow.

Local filtering excludes obvious promotions, security notices, receipts, newsletters, and mail without job indicators. Each network request contains one email; there are no automatic HTTP retries. An unchanged content hash reuses the validated result even after switching models or restarting JobView. Previously attempted requests with no valid result are skipped, preventing automatic recharges after a lost response or rate-limit failure.

**Request fresh results** explicitly permits resending selected candidates, including prior successes and failures. It starts with the newest candidates each time and respects the request cap; increase the cap if you deliberately want to resend a larger set. Changing the model alone does not resend cached content.

The per-run cap is not a monthly spending ceiling. Google Cloud budgets are alerts, not hard cutoffs. Settings records attempts, successful responses, cache hits, and successful-response token counts. Failed/cancelled calls may have additional usage that JobView could not read; Google's billing console is authoritative. No aggregate dollar total is inferred from mixed-model token counters.

## What leaves the computer

Only the normalized subject (up to 500 characters), sender name (up to 200), sender domain, and up to 8,000 body characters are sent from Rust directly to Google's Gemini endpoint over HTTPS. Sender mailbox local parts, recipient headers, Gmail IDs, dates, raw MIME/HTML, and attachments are omitted. Recognized quoted history is removed during normalization; URLs and email addresses are redacted before the request. This is minimization, not anonymization: names and other personal information may remain in prose, and unusual quoted history may not be recognized.

The API key briefly crosses the local IPC boundary from the masked input; it is never returned to JavaScript, persisted in browser storage/SQLite, or logged. The webview cannot call Google directly. JobView uses no grounding, tools, attachment upload, or remote email images. Sending email text to Gemini never changes Gmail messages.

## How results are handled

Gemini is prompted to distinguish personal recruitment from course sales, general job alerts, and account/password messages, including Swedish phrasing and negation. It must return a fixed JSON structure with known categories, events, stages, and reasoning codes. Missing information must be null, and deadlines require a complete explicit date. Invalid, truncated, blocked, or inconsistent responses leave the existing classification and application state intact.

Validated model scores below 0.50 are cached but do not replace the current classification. Other valid results update classification fields and existing linked-email evidence atomically. They never create jobs or link messages automatically. Manual corrections, links, stages, notes, and completed actions remain protected. A changed or ignored email invalidates a pending result. Model scores are self-reported, not measured probabilities.

**Rebuild local classifications** updates local-rule results and preserves existing Gemini classifications. Marking mail not job related or ignoring it prevents future bulk AI review. Candidate filtering can still miss unusual messages, so this is not a promise of perfect recall. Automatic job matching remains a separate future feature.

## Troubleshooting

- **Key rejected / permissions:** check that you copied the entire current key and that its project has Gemini API access. Replace or rotate the key in AI Studio, then save it in JobView.
- **Key format rejected:** copy the complete key from AI Studio without surrounding spaces. JobView accepts punctuation and long authorization keys; it does not require a legacy key prefix. See [Google's key documentation](https://ai.google.dev/gemini-api/docs/api-key).
- **Quota / rate limit:** inspect the project's quotas and billing. After resolving the limit, use the explicit fresh-results option to retry previously failed candidates; repeated normal reviews skip their attempted hashes.
- **Invalid or incomplete response:** local state is preserved. Inspect the email manually or request a fresh result; try the newer model if repeated classification failures occur.
- **Secure storage unavailable:** Windows Credential Manager must work. JobView has no plaintext fallback. Run the normal desktop app as the same Windows user.
- **No new requests:** verify that the AI mode was saved, a key is configured, and eligible uncached candidates exist. Manual overrides, obvious exclusions, cached results, and unsuccessful prior attempts are intentionally skipped.

Automated tests use fictional text and simulated provider responses. A successful live connection test and review of representative Swedish mail require your own key and remain part of release acceptance.
