//! SMS alternative for Core User's step-up provider. No Bilimbi module mirrors this file.
use super::{clear_failures, refuse_impersonation, TwoFactorError};
use crate::{password, UserService};
use bilibi_database::{Change, Error, Result};
use bilibi_settings::Target;
use bilibi_tenancy::Scope;
use serde_json::json;

pub const ENABLED_KEY: &str = bilibi_step_up::SMS_ENABLED_KEY;
const PURPOSE_PHONE: &str = "core.user.phone.verify";
const TARGET_PHONE: &str = "own-phone";
const PURPOSE_BROWSER: &str = "core.user.step_up";
const TARGET_BROWSER: &str = "browser-session";
const MAX_ATTEMPTS: i16 = 5;

#[derive(Clone, Debug, Default)]
pub struct SmsFactorStatus {
    pub company_enabled: bool,
    pub phone_number: Option<String>,
    pub phone_verified: bool,
    pub masked_phone: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SmsChallengeSent {
    pub masked_phone: String,
    pub retry_after_seconds: i64,
}

pub(crate) fn mask_phone(phone: &str) -> String {
    let visible = phone.chars().rev().take(3).collect::<Vec<_>>();
    let suffix: String = visible.into_iter().rev().collect();
    let prefix: String = phone.chars().take(3).collect();
    format!("{prefix} ••• ••• {suffix}")
}

fn fresh_code() -> Result<String> {
    let mut bytes = [0_u8; 4];
    getrandom::fill(&mut bytes).map_err(|_| Error::Internal)?;
    Ok(format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000))
}

impl UserService {
    pub async fn sms_factor_status(&self, scope: &Scope) -> Result<SmsFactorStatus> {
        let client = self.db.get().await?;
        let company_enabled = match scope.actor().organisation() {
            Some(company) => self
                .settings
                .get(&**client, scope, Target::Company(company), ENABLED_KEY)
                .await?
                .value
                .as_bool()
                .unwrap_or(false),
            None => false,
        };
        let row = client
            .query_opt(
                "SELECT phone_number,phone_verified_at IS NOT NULL FROM users WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL",
                &[&scope.tenant(), &scope.user()],
            )
            .await?;
        let Some(row) = row else {
            return Err(Error::NotFound);
        };
        let phone_number: Option<String> = row.get(0);
        let phone_verified: bool = row.get(1);
        Ok(SmsFactorStatus {
            company_enabled,
            masked_phone: phone_number.as_deref().map(mask_phone),
            phone_number,
            phone_verified,
        })
    }

    pub async fn sms_enrolled(&self, scope: &Scope) -> Result<bool> {
        let status = self.sms_factor_status(scope).await?;
        Ok(status.company_enabled && status.phone_verified && status.phone_number.is_some())
    }

    pub async fn request_phone_verification(
        &self,
        scope: &Scope,
    ) -> std::result::Result<SmsChallengeSent, TwoFactorError> {
        self.send_sms_challenge(scope, PURPOSE_PHONE, TARGET_PHONE, false)
            .await
    }

    pub async fn confirm_phone(
        &self,
        scope: &Scope,
        code: &str,
    ) -> std::result::Result<(), TwoFactorError> {
        self.verify_sms_challenge(scope, code, PURPOSE_PHONE, TARGET_PHONE, false, true)
            .await
            .and_then(|accepted| accepted.then_some(()).ok_or(TwoFactorError::InvalidCode))
    }

    pub async fn send_step_up_sms(
        &self,
        scope: &Scope,
        operation: &str,
        target: &str,
    ) -> std::result::Result<SmsChallengeSent, TwoFactorError> {
        self.send_sms_challenge(scope, operation, target, true)
            .await
    }

    pub async fn request_browser_step_up_sms(
        &self,
        scope: &Scope,
    ) -> std::result::Result<SmsChallengeSent, TwoFactorError> {
        self.send_sms_challenge(scope, PURPOSE_BROWSER, TARGET_BROWSER, true)
            .await
    }

    pub async fn step_up_with_sms(
        &self,
        scope: &Scope,
        code: &str,
    ) -> std::result::Result<(), TwoFactorError> {
        self.verify_sms_challenge(scope, code, PURPOSE_BROWSER, TARGET_BROWSER, true, false)
            .await
            .and_then(|accepted| accepted.then_some(()).ok_or(TwoFactorError::InvalidCode))
    }

    pub(crate) async fn verify_step_up_sms(
        &self,
        scope: &Scope,
        code: &str,
        operation: &str,
        target: &str,
        browser: bool,
    ) -> std::result::Result<bool, TwoFactorError> {
        self.verify_sms_challenge(scope, code, operation, target, browser, false)
            .await
    }

    async fn send_sms_challenge(
        &self,
        scope: &Scope,
        purpose: &str,
        target: &str,
        require_verified: bool,
    ) -> std::result::Result<SmsChallengeSent, TwoFactorError> {
        refuse_impersonation(scope)?;
        if purpose.is_empty() || purpose.len() > 128 || target.is_empty() || target.len() > 512 {
            return Err(Error::Invalid.into());
        }
        let status = self.sms_factor_status(scope).await?;
        if !status.company_enabled {
            return Err(TwoFactorError::SmsDisabled);
        }
        let phone = status.phone_number.ok_or(TwoFactorError::PhoneMissing)?;
        if require_verified && !status.phone_verified {
            return Err(TwoFactorError::PhoneUnverified);
        }
        let code = fresh_code()?;
        let code_hash = password::hash(&code).await?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (tenant, user) = (scope.tenant(), scope.user());
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("user.two_factor.sms.prepared"),
            )
            .await?;
        let scope = self.fresh(tx.sql(), scope).await?;
        let phone_for_insert = phone.clone();
        let purpose_owned = purpose.to_owned();
        let target_owned = target.to_owned();
        let id_for_insert = id.clone();
        tx.mutate(move |sql| {
            Box::pin(async move {
                sql.query_one(
                    "SELECT id FROM users WHERE tenant_id=$1 AND id=$2 AND phone_number=$3 AND deleted_at IS NULL FOR UPDATE",
                    &[&tenant, &user, &phone_for_insert],
                )
                .await?;
                let latest: Option<i64> = sql
                    .query_opt(
                        "SELECT ceil(60-extract(epoch FROM now()-created_at))::bigint FROM user_sms_challenges WHERE tenant_id=$1 AND user_id=$2 AND purpose=$3 AND created_at>now()-interval '60 seconds' ORDER BY created_at DESC LIMIT 1",
                        &[&tenant, &user, &purpose_owned],
                    )
                    .await?
                    .map(|row| row.get(0));
                if latest.is_some_and(|seconds| seconds > 0) {
                    return Err(Error::Conflict);
                }
                let sent: i64 = sql
                    .query_one(
                        "SELECT count(*) FROM user_sms_challenges WHERE tenant_id=$1 AND user_id=$2 AND purpose=$3 AND created_at>now()-interval '1 hour'",
                        &[&tenant, &user, &purpose_owned],
                    )
                    .await?
                    .get(0);
                if sent >= 5 {
                    return Err(Error::Invalid);
                }
                let invalidated = sql.execute(
                    "UPDATE user_sms_challenges SET status='invalid' WHERE tenant_id=$1 AND user_id=$2 AND purpose=$3 AND target=$4 AND status IN ('pending','sent')",
                    &[&tenant, &user, &purpose_owned, &target_owned],
                )
                .await?;
                sql.execute(
                    "INSERT INTO user_sms_challenges(id,tenant_id,user_id,purpose,target,phone_number,code_hash,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,now()+interval '5 minutes')",
                    &[&id_for_insert, &tenant, &user, &purpose_owned, &target_owned, &phone_for_insert, &code_hash],
                )
                .await?;
                let mut changes = vec![Change {
                        kind: "user_sms_challenge".into(),
                        id: user,
                        old: json!({}),
                        new: json!({"status":"prepared","purpose":purpose_owned}),
                    }];
                if invalidated > 0 {
                    changes.push(Change {
                        kind: "user_sms_challenge".into(),
                        id: user,
                        old: json!({"status":"active"}),
                        new: json!({"status":"invalid","reason":"resent"}),
                    });
                }
                Ok(((), changes))
            })
        })
        .await
        .map_err(|error| match error {
            Error::Conflict => TwoFactorError::ResendThrottled { seconds: 60 },
            Error::Invalid => TwoFactorError::HourlyLimit,
            other => TwoFactorError::Other(other),
        })?;
        tx.commit().await?;

        let message = bilibi_sms::Message::new(
            &phone,
            format!("Your Bilibi verification code is {code}. It expires in 5 minutes."),
        )
        .map_err(|_| TwoFactorError::SmsUnavailable)?;
        let delivered = self.sms.deliver(&message).await;
        let mut client = self.db.get().await.map_err(Error::from)?;
        let event = if delivered.is_ok() {
            "user.two_factor.sms.sent"
        } else {
            "user.two_factor.sms.delivery_failed"
        };
        let mut tx = self
            .mutations
            .begin(&mut client, scope.mutation_context(event))
            .await?;
        let sent = delivered.is_ok();
        let id_for_update = id.clone();
        tx.mutate(move |sql| {
            Box::pin(async move {
                let status = if sent { "sent" } else { "invalid" };
                let changed = sql
                    .execute(
                        "UPDATE user_sms_challenges SET status=$2,sent_at=CASE WHEN $3 THEN now() ELSE NULL END WHERE id=$1 AND status='pending'",
                        &[&id_for_update, &status, &sent],
                    )
                    .await?;
                if changed != 1 {
                    return Err(Error::Conflict);
                }
                Ok((
                    (),
                    vec![Change {
                        kind: "user_sms_challenge".into(),
                        id: user,
                        old: json!({"status":"prepared"}),
                        new: json!({"status":status}),
                    }],
                ))
            })
        })
        .await?;
        tx.commit().await?;
        if delivered.is_err() {
            return Err(TwoFactorError::SmsUnavailable);
        }
        Ok(SmsChallengeSent {
            masked_phone: mask_phone(&phone),
            retry_after_seconds: 60,
        })
    }

    async fn verify_sms_challenge(
        &self,
        scope: &Scope,
        code: &str,
        purpose: &str,
        target: &str,
        open_window: bool,
        verify_phone: bool,
    ) -> std::result::Result<bool, TwoFactorError> {
        refuse_impersonation(scope)?;
        let code = code.trim();
        if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            return Ok(false);
        }
        let (tenant, user) = (scope.tenant(), scope.user());
        let client = self.db.get().await.map_err(Error::from)?;
        let row = client
            .query_opt(
                "SELECT id,code_hash,expires_at>now() FROM user_sms_challenges WHERE tenant_id=$1 AND user_id=$2 AND purpose=$3 AND target=$4 AND status='sent' ORDER BY created_at DESC LIMIT 1",
                &[&tenant, &user, &purpose, &target],
            )
            .await
            .map_err(Error::from)?;
        let Some(row) = row else {
            return Ok(false);
        };
        let id: String = row.get(0);
        let hash: String = row.get(1);
        let current: bool = row.get(2);
        if !current {
            drop(client);
            let mut client = self.db.get().await.map_err(Error::from)?;
            let mut tx = self
                .mutations
                .begin(
                    &mut client,
                    scope.mutation_context("user.two_factor.sms.expired"),
                )
                .await?;
            let expired_id = id.clone();
            tx.mutate(move |sql| {
                Box::pin(async move {
                    sql.execute(
                        "UPDATE user_sms_challenges SET status='invalid' WHERE id=$1 AND status='sent' AND expires_at<=now()",
                        &[&expired_id],
                    )
                    .await?;
                    Ok((
                        (),
                        vec![Change {
                            kind: "user_sms_challenge".into(),
                            id: user,
                            old: json!({"status":"sent"}),
                            new: json!({"status":"invalid","reason":"expired"}),
                        }],
                    ))
                })
            })
            .await?;
            tx.commit().await?;
            return Ok(false);
        }
        let matched = password::verify(code, Some(&hash)).await?;
        drop(client);
        let event = if matched {
            if verify_phone {
                "user.phone.verified"
            } else {
                "user.two_factor.sms.verified"
            }
        } else {
            "user.two_factor.sms.failed"
        };
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(&mut client, scope.mutation_context(event))
            .await?;
        let session_scope = scope.clone();
        let sessions = self.sessions.clone();
        let id_for_update = id.clone();
        let outcome = tx
            .mutate(move |sql| {
                Box::pin(async move {
                    if matched {
                        let used = sql
                            .execute(
                                "UPDATE user_sms_challenges SET status='used',used_at=now() WHERE id=$1 AND tenant_id=$2 AND user_id=$3 AND status='sent' AND expires_at>now()",
                                &[&id_for_update, &tenant, &user],
                            )
                            .await?;
                        if used != 1 {
                            return Err(Error::Conflict);
                        }
                        if verify_phone {
                            sql.execute(
                                "UPDATE users SET phone_verified_at=now(),version=version+1,updated_at=now() WHERE tenant_id=$1 AND id=$2 AND phone_number=(SELECT phone_number FROM user_sms_challenges WHERE id=$3)",
                                &[&tenant, &user, &id_for_update],
                            )
                            .await?;
                        }
                        if open_window {
                            sessions.step_up(sql, &session_scope).await?;
                        }
                        clear_failures(sql, tenant, user).await?;
                        let mut changes = vec![Change {
                                kind: "user_sms_challenge".into(),
                                id: user,
                                old: json!({"status":"sent"}),
                                new: json!({"status":"used","purpose":purpose}),
                            }];
                        if verify_phone {
                            changes.push(Change {
                                kind: "user".into(),
                                id: user,
                                old: json!({"phone_verified":false}),
                                new: json!({"phone_verified":true}),
                            });
                        }
                        Ok((true, changes))
                    } else {
                        let row = sql
                            .query_opt(
                                "UPDATE user_sms_challenges SET attempts=attempts+1,status=CASE WHEN attempts+1>=5 THEN 'invalid' ELSE status END WHERE id=$1 AND tenant_id=$2 AND user_id=$3 AND status='sent' AND expires_at>now() RETURNING attempts,status",
                                &[&id_for_update, &tenant, &user],
                            )
                            .await?;
                        let Some(row) = row else {
                            return Ok((false, Vec::new()));
                        };
                        let attempts: i16 = row.get(0);
                        let status: String = row.get(1);
                        Ok((
                            false,
                            vec![Change {
                                kind: "user_sms_challenge".into(),
                                id: user,
                                old: json!({"status":"sent"}),
                                new: json!({"status":status,"attempts":attempts}),
                            }],
                        ))
                    }
                })
            })
            .await;
        match outcome {
            Ok(true) => {
                tx.commit().await?;
                Ok(true)
            }
            Ok(false) => {
                tx.commit().await?;
                let client = self.db.get().await.map_err(Error::from)?;
                let locked = client
                    .query_opt(
                        "SELECT 1 FROM user_sms_challenges WHERE id=$1 AND status='invalid' AND attempts>=$2",
                        &[&id, &MAX_ATTEMPTS],
                    )
                    .await
                    .map_err(Error::from)?
                    .is_some();
                if locked {
                    Err(TwoFactorError::SmsLocked)
                } else {
                    Ok(false)
                }
            }
            Err(Error::Conflict) => {
                tx.rollback().await?;
                Ok(false)
            }
            Err(error) => Err(error.into()),
        }
    }
}
