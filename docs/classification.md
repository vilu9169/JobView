# English and Swedish classification

JobView classifies cached email locally using deterministic English and Swedish rules. No account or API key is required for these rules. Classifications explain the detected category, recruitment event, and action signals; they do not automatically associate mail with a job.

## Rebuild existing mail

Choose **Settings → Rebuild local classifications → Rebuild** to apply the current rules to cached originals without downloading Gmail bodies again. Newly imported mail uses the current rules immediately. A normal incremental sync does not reclassify unchanged cached messages.

Rebuilding preserves manual classifications, manual links/stages, notes, completed actions, and ignored emails. Incorrect inferred events are superseded while their audit records remain. Manually linked mail stays linked even when its new classification is not job related; you retain control of that association. Rebuilding also refreshes normalized text, snippets, and hashes for non-overridden emails.

## Supported signals and exclusions

- Swedish confirmations, personal recruiter outreach, interview invitations/confirmations/final interviews, assessments/completions, employment offers, and rejections.
- Swedish company/role labels and conservative subject extraction, plus explicit deadlines in ISO, Swedish day/month/year, and written-month formats. Ambiguous dates without a year are left unset.
- Dedicated account/security exclusions for password resets, verification, and account activation, even from recruiting senders.
- Promotion, newsletter, job-alert, and receipt exclusions in both languages. Generic advice about “din ansökan” does not establish a personal application update.
- Recruiter contact requires personal outreach evidence. A recruiting sender, signature, or the word “recruiter” alone is insufficient.
- Tracking URLs are ignored for classification signals. HTML incorrectly labeled as plain text is converted locally, and Swedish quoted-history delimiters are recognized.
- Email details describe confidence as **rule strength**, a heuristic score rather than a measured probability.

Tests include a course-sale promotion and an ATS password-reset notification. Both appear in Other Mail and produce no job event or job action. Test messages use shortened fictional examples, reserved example domains, and no real reset links or tracking identifiers.

## Limits and optional AI

Rule strength is a heuristic score, not a measured probability. Real-mail precision and recall have not been measured, and unusual wording can be missed. [Optional Gemini classification](gemini-setup.md) can review uncertain emails or a broader set of job candidates. Local rebuilds preserve existing Gemini results as well as manual classifications. Automatic job association remains a separate future feature.
