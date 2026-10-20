# ADR 03: Pluggable second factors and SMS

**Document Type:** Architecture Decision Record
**Status:** Accepted. TOTP, SMS, company policy, and pending-login isolation are built; the current Core User design uses administrator resets instead of recovery codes; prompt-injection-specific agent windows remain deferred.
**Agents:** codex/gpt-5 (author)
**Scope:** How Bilibi chooses and verifies second-factor methods for browser, API, MCP and future CLI callers
**Last Updated:** 2026-10-20

## Context

Step-up originally assumed one authenticator-app code. Companies also need SMS, login must be able to consume the same policy as any other operation, and an agent must receive the same answer as a browser without a method-specific business action.

SMS adds delivery secrets, phone ownership, expiring challenges, resend limits and a dangerous login boundary. A password-correct browser must not receive a usable session token before its second proof succeeds.

## Decision

### Multiple proof methods

Core User remains the one Step-up provider. It reports the methods available to the account and accepts TOTP or an SMS code. Codes are bound to the account and purpose; an agent SMS code is additionally bound to the immutable plan code.

`base/sms` owns the delivery contract: `Sms`, `SmsGateway`, `Message`, `DeliveryOutcome` and `SERVICE`. Development uses its bounded private simulator inbox. Staging and production use Twilio through Base Integration, with payload retention off and encrypted global Settings for the account SID, auth token and sender number. Incomplete configuration is unavailable, never a partial send.

### Company policy

Operation modes, the 30-minute sliding window, the six-hour hard cap and the SMS enable switch are company settings. A company value wins; old tenant values remain a read-only fallback until that company saves its own value. Installation floors and maxima remain global.

`core.user.login` is a discovered operation and is off by default. Existing role and capability changes keep their existing on-by-default declarations. The same Step-up facade and policy apply to GUI, API, MCP and future CLI callers.

### Phone and challenge security

An administrator may store an account phone in strict E.164 form. The account holder verifies it by SMS. Changing or clearing it clears verification and invalidates outstanding challenges.

SMS codes are six random decimal digits, stored only as Argon2id hashes, expire after five minutes, work once and lock after five wrong attempts. Sending is transactionally limited to one per 60 seconds and five per rolling hour for each account and purpose. A resend invalidates the earlier code. Audit records lifecycle outcomes without codes, message bodies, credentials, session IDs or handles.

### Pending-login isolation

Password verification creates the normal session row but does not give its token to the browser when login step-up applies. The browser receives a random five-minute handle whose hash references the server-held session. Ordinary session guards cannot derive or accept that handle. Verification consumes the proof and handle before the normal cookie is issued; cancellation or expiry deletes the pending handle and unused session.

If no usable factor exists, sign-in stays blocked and tells the person to contact an administrator. It never silently authenticates or enrolls a method.

### Four-door behavior

The browser offers authenticator and SMS on one challenge. API and MCP commits accept optional `step_up_method: "sms"`: without a code it requests delivery and returns `step_up_required` with methods, masked destination and resend timing; the caller repeats the unchanged plan with `step_up_code`. Existing authenticator-only callers remain compatible.

Development routes have their own contribution catalog. The public `/dev/sms-inbox` route is therefore not bound at all outside development, which lets a tester read a login code without weakening a deployed server.

## Consequences

- Business modules still know only operation keys and `StepUp::require`; they do not import SMS or User.
- Twilio credentials are operator settings rather than environment-only configuration, and secret values are never shown back.
- Login uses the same operation catalog, policy, proof window and audit language as every other protected action.
- SMS is a weaker and cost-bearing factor, so each company must opt in and each phone must be verified.
- Prompt injection that tricks an agent into spending a still-open window remains a separate design problem. The current protection is the immutable reviewed plan, per-purpose proof binding and six-hour cap; this ADR does not claim to solve instruction provenance.

## References

- [`apps/base/step_up/README.md`](../../../apps/base/step_up/README.md)
- [`apps/base/sms/README.md`](../../../apps/base/sms/README.md)
- [`apps/core/user/README.md`](../../../apps/core/user/README.md)
- [ADR 01](./01-agent-first-operations.md)
- [ADR 02: Build in development and keep everything in step](./02-build-in-development-keep-everything-in-step.md)
