# User credentials

`core/user` owns accounts, credentials and the notification bell. This slice
adds the password and email services that the login, forgot-password,
reset-password and settings pages call; the pages are not part of it.

## Services (`core/user.accounts`, `UserService`)

| Method | Purpose |
|---|---|
| `login` / `logout` | Tenant + email credential lookup and server session (unchanged) |
| `change_password(token, csrf, current, new)` | Signed-in change. Needs the current password; ends the account's other sessions |
| `request_password_reset(tenant, email)` | Always `Ok` for unknown, frozen or throttled accounts. Mail is sent after returning |
| `reset_password(tenant, email, token, new)` | Consumes the one-time token, replaces the password, ends every session |
| `send_email_verification(token, csrf)` / `verify_email(link)` | Signed link to the account's current address; clicking proves mailbox control |

Refusals are `CredentialError` variants a page maps to a field error or flash
(`IncorrectPassword`, `Policy`, `InvalidOrExpiredToken`, ...). Unknown, frozen,
expired, reused and wrong-email tokens are one indistinguishable
`InvalidOrExpiredToken`.

Use `password::{hash, verify, check_policy}` for any password work instead of
calling `argon2` directly. It runs Argon2id on the blocking pool, bounds input
(8 characters to 1024 bytes) and `verify(.., None)` costs the same as a real
check, so timing does not reveal which emails exist.

## Token storage

`password_reset_tokens` has one row per account (primary key tenant + user), so
it can never grow past the user count. Only the SHA-256 of the 256-bit random
token is stored; the plaintext exists in the emailed link alone. A request is
throttled to one per 60 seconds, a link lives one hour, and each request
deletes expired rows. Consuming a token deletes its row inside the audited
transaction, so a racing second use loses. If delivery fails the row is
removed so the person can ask again.

Verification links are stateless: `v1.{tenant}.{user}.{issued}.{hmac}` signed
with `BILIBI_USER_SIGNING_KEY` (64 hex characters; separate from the Settings
and Media keys). The MAC covers the current email, so changing the address voids
links already sent. Without the key `send_email_verification` and `verify_email`
answer `VerificationUnavailable`.

Emailed links point at `APP_ORIGIN`: `/reset-password/{token}?tenant=…&email=…`
and `/verify-email/{link}`. The pages that serve them belong to later slices.

## Audit

Credential writes are audited as kind `user` with marker values
(`"password_hash":"replaced"`), never a hash; the audit layer redacts the field
name as well. Reset and verification are attributed to the account that proved
the token.

## Known gaps

Identity is still tenant + email, and the archived-company freeze applies only
through tenant and account status. The global login and Person/Affiliation
binding follow K9. No throttle exists on guessing a password at login.

## Walkthrough

`cargo run -p bilibi-user --example credentials-walkthrough` against a
disposable database after `migrate` and `seed` (set
`BILIBI_DISPOSABLE_CREDENTIALS_DEMO=yes`, `BILIBI_MAIL_DIR` and
`BILIBI_USER_SIGNING_KEY`). It prints every outcome above and reads the dev
mailbox to follow the links.
