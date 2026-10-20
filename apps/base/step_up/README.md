# Base Step-up

Ask the human account holder to prove it is them again before an operation proceeds on any channel. No Bilimbi counterpart.

- This module names no page, record or second-factor implementation.
- Each owner declares its operation and calls `StepUp::require` from its facade.
- Core User binds the provider for authenticator and SMS proofs.

## Declare and require

```rust
bilibi_step_up::declare!(
    "domain/module",
    bilibi_step_up::Operation::new("domain.module.thing.approve", "Approve a thing")
);

self.step_up.require(scope, KEY, &format!("thing:{id}")).await?;
```

Keys are lower case and start with the owning module ID with `/` written as `.`. The target names the action or record in audit data and must contain no secret.

- `.off_by_default()` lets each company opt in.
- `.floor()` is on by default and the company cannot turn it off while the installation floor applies.
- `.always()` requires a fresh proof for every action on every environment.
- `.guards_setting("key")` applies the same operation before that setting changes.
- Return `Error::StepUpRequired` unchanged; each door maps it to its own response.
- Never decide in business code whether a proof applies. That is company policy.

## Company policy

Companies configure **Administration > Authorization > Step-up 2FA** at `/authz/step-up` with `admin.authz.step-up.manage`. Values previously stored at tenant scope remain a read-only fallback until the company saves its own value.

| Setting | Scope | Default | Bounds |
| --- | --- | --- | --- |
| Per operation: off, on, ask every time | Company (`step_up.policy`) | Declaration | - |
| Sliding window | Company (`step_up.window_minutes`) | 30 minutes | 5-720 |
| Hard cap | Company (`step_up.cap_hours`) | 6 hours | 1-24 and at least the window |
| SMS proof method | Company (`two_factor.sms_enabled`) | Off | - |
| Operations companies cannot turn off | Installation (`step_up.floor`) | `.floor()` declarations | - |
| Longest window and cap | Installation | 720 minutes / 24 hours | Same bounds |

Changing policy is itself guarded by `base.step_up.policy.update`, capability checked in the mutation transaction and audit logged. Development turns ordinary enforcement off; `.always()` still applies.

## Proofs and windows

| Channel | Proof | Window |
| --- | --- | --- |
| Browser | Authenticator or requested SMS on the confirm page | Session row |
| API/MCP | `step_up_code` on the commit; optional `step_up_method: "sms"` requests delivery | Agent connection and holder |

The window slides after each protected action. The cap counts from the accepted proof and always wins. “Ask every time” spends one fresh proof per action. Signing the holder out everywhere also ends agent windows.

A presented code is verified and consumed for the first operation and target that uses it, then covers the rest of that request only. Agent SMS challenges use the immutable plan code as their target. Impersonation cannot satisfy step-up.

## Agent flow

1. Commit the reviewed plan. A protected operation returns `step_up_required`.
2. Ask for an authenticator code, or repeat the unchanged request with `step_up_method: "sms"` and no code.
3. An SMS request returns available methods, a masked destination and resend timing.
4. Repeat the unchanged plan with `step_up_code`; keep `step_up_method: "sms"` when presenting an SMS code.

Existing callers that only provide `step_up_code` remain compatible.

## Provider contract

`StepUp::bind` runs once at boot. The provider answers:

| Method | Answer |
| --- | --- |
| `enrolled` | Whether any confirmed second factor exists |
| `methods` | Usable methods and masked SMS destination |
| `request_sms` | Request an SMS challenge for a `Purpose` |
| `session` | Current browser window or fresh check |
| `spend` | Verify and consume a code for operation, target and channel |
| `prompt` | Browser confirmation route |

## Audit

Applied checks record `step_up.passed`, `step_up.refused` or `step_up.failed` with operation, module, target, channel and mode. Provider lifecycle events are owned by Core User. Codes, secrets, SMS bodies, credentials, session tokens and pending-login handles are never recorded.

See [ADR 03](../../../docs/architecture/decisions/03-pluggable-second-factors.md).
