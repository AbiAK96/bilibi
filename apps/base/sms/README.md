# SMS

Send a validated text message through one installation-selected transport. There is no Bilimbi counterpart.

- Resolve `base/sms.sms` and call `Sms::deliver`; callers never call Twilio or write simulator files themselves.
- `BILIBI_ENV=development` selects the bounded simulator inbox. Every other environment selects Twilio.
- Twilio credentials are encrypted global Settings values owned by this module. Missing configuration returns `Unavailable`; it never falls back to the simulator outside development.
- The transport records a payload-free `base/integration` exchange. Authorization headers, message bodies and verification codes are never retained.
- The simulator stores at most 100 JSON messages under `BILIBI_SMS_DIR` (default `.local/sms-inbox`) with private directory/file permissions where the operating system supports them.

The facade accepts an already composed body and an E.164 recipient. Phone ownership, challenges, throttling and audit belong to the calling module.
