# Mail

`base/mail` owns outgoing email: one validated `Message`, one `Mailer` service
(`base/mail.mailer`) and two transports. Owners build a `Message` and call
`Mailer::deliver`; they never open an SMTP connection, write `.eml` files or
compose headers themselves, because `Message::render` is the only place the
recipient, subject and body are checked against header injection.

## Choosing the transport

Decided once at boot by `Mailer::from_env`:

| Environment | Transport |
|---|---|
| `MAIL_HOST` unset | **Dev mailbox**: each message is one file in `BILIBI_MAIL_DIR` (default `.local/mailbox`) |
| `MAIL_HOST` set | **SMTP relay** with verified TLS. `MAIL_PORT`, `MAIL_USERNAME`, `MAIL_PASSWORD`, `MAIL_TLS_MODE` (`starttls` or `implicit_tls`), `MAIL_FROM_NAME` and `MAIL_FROM_ADDRESS` are then all required |

A half-configured relay refuses boot with the missing variable named; it never
falls back to files. `MAIL_FROM_NAME`/`MAIL_FROM_ADDRESS` default to
`Bilibi <noreply@bilibi.localhost>` for the dev mailbox only. The boot log
prints which transport is active, never credentials.

## Dev mailbox

Open the newest file in `.local/mailbox/` to read a reset or verification link
without a mail server. Files are stored decoded (`8bit`) so a link can be
copied directly, the directory is `0700` and files `0600` (they hold live
tokens), and only the newest 100 are kept. `Mailbox::entries` and
`Mailbox::read` list and return them for a future preview page; `read` accepts
only names it generated.

## Delivery contract

`deliver` returns `Error::Delivery(detail)` with a transport detail meant for
logs, not for users. A caller that must not reveal whether an address exists
sends after answering and undoes what depended on delivery when it fails;
`core/user`'s forgotten-password request is the reference.
