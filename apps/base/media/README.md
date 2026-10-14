# Media storage contract (M1)

`base/media` owns private asset storage: operator-mapped disks, asset metadata
(`base_media_assets`), content-checked uploads, signed single-asset streams and
the trusted subject-owner seam. Private attachment state, ULIDs and cleanup are
M2; this crate leaves the seam they need (`NewAsset::private`, `SubjectOwner`).

Owners call `base/media.store` (`Media`) **after** authorizing their own
workflow. Nothing here is a browser endpoint except the signed stream route the
Web host mounts at `/media/assets/{id}/stream`.

## Operator setup

Settings (global scope, editor group Media, capability
`admin.system.media.manage`): `media.disks` maps durable disk names to existing
absolute directories with mode `0700`, not under a `static`, `public` or
`assets` path and with no symlink anywhere in the path. `media.max_bytes`
(default 10 MiB, at most 512 MiB), `media.attachment_allowed_types`,
`media.attachment_disk` and `media.stream_ttl_seconds` (default 300).
Keep disk roots stable while assets reference them; `external` is a reserved
sentinel for linked (not stored) assets and cannot be a disk name.

Boot with `BILIBI_MEDIA_SIGNING_KEY` set to 64 hex characters (32 random
bytes). It is separate from `BILIBI_SETTINGS_KEY`. Without it, uploads still
work but signing and verifying refuse (`Error::SigningUnavailable`). The host
also needs the `file` command with its libmagic database; without it uploads
refuse with `MimeDetectionUnavailable` instead of trusting the client.

## Writing assets

- `put_upload(&mut client, scope, disk, directory, Upload, metadata)` checks
  size, detects the MIME type from the bytes (never the filename or a claimed
  content type), refuses empty files and active content, writes
  `tenants/{tenant}/{directory}/{uuid}` exclusively with mode `0600`, then
  registers it in its own audited transaction. A failed registration deletes the
  file. Active types are HTML, XHTML, XML, SVG and JavaScript MIME types. libmagic
  cannot tell script from prose, so script-like text is stored as `text/plain`;
  what protects it is delivery (below), not detection.
- `put_derivative_bytes` stores a derivative under an existing original with the
  type detected the same way.
- `register_original` / `register_derivative` record bytes an authorized owner
  already placed, inside the owner's `MutationTransaction`; they touch no file.
- Calls that write bytes take a plain `&mut Client` on purpose: files and rows
  cannot share a rollback, so Media owns the transaction. Do not start
  a transaction you might roll back around a file write.
- `delete_asset` commits the row deletions first (tree, children first), then
  removes files. A file that cannot be removed reports `CleanupFailed`; the
  operator reconciles the disk.
- Every row is tenant-scoped through the sealed `Scope`; another tenant's id is
  `NotFound`. Audit records name the asset but never its storage key or metadata.

## Signed streams

`stream_url` / `display_url` return `/media/assets/{id}/stream?token=...` where
the token is `v1.{tenant}.{expires}.{hmac}` binding asset id, tenant and expiry.
The route has no session: the token alone authorizes that one public asset until
it expires. Verification is constant-time, then the row, its tenant, the
private-tree rule and the disk are rechecked. Invalid, expired or private-bound
requests answer 403; missing or unreadable assets 404.

Only JPEG, PNG, GIF and WebP render inline. Everything else is an
`application/octet-stream` attachment. Every stream sends `nosniff`,
`Content-Security-Policy: default-src 'none'; sandbox` and `private, no-store`;
build headers with `StreamFile::delivery`, never by hand. The Web host's
default security headers only fill gaps, so these win.

An asset in a tree containing a `private` row (a private attachment, M2) is
refused by streams, derivatives, listing and deletion in either direction.

## Subject owners

A module that needs private files for its records depends on `base/media` and
contributes `Subject { owner: &OWNER }` under a stable lowercase key
(`orders.order`) to `SUBJECTS`. `Media::authorize_subject` calls the owner's
`SubjectOwner::authorize` with the caller's sealed scope and a `SubjectAction`.
The owner resolves its live record and checks its own capability; unknown
subjects have no access. Contributions from modules that do not depend on
`base/media` fail at boot.

## Manual walkthrough

On a disposable database run the host's `migrate` and `seed`. Set
`DATABASE_URL`, `BILIBI_DISPOSABLE_MEDIA_DEMO=yes`, `BILIBI_MEDIA_SIGNING_KEY`
and `BILIBI_MEDIA_DEMO_ROOT` (a scratch directory the command makes `0700`), then
run `cargo run -p bilibi-media --locked --example media-walkthrough`. It appoints
tenant 1 operator, stores a PNG and a note, shows rejected active-content,
oversize, empty and unsafe uploads, derivative and private-tree rules, tenant
isolation, token refusals and subject owners, and prints signed paths. Start the
app with the same signing key and `curl` those paths; run again with
`-- delete <id>` for a separate-process delete. It is a demonstration, not a
test harness; its fixture subject owner lives only in the example executable.
