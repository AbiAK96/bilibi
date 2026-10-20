//! The authenticator as Base Step-up's provider: Step-up asks, this answers
//! about the scope's own account holder. It learns only an operation key and
//! target as data to record, never what the operation is.
use super::{Spend, TwoFactorError};
use crate::UserService;
use bilibi_step_up::{
    AvailableMethods, Provider, Purpose, SessionProof, SmsRequest, Spent, WindowState,
};
use bilibi_tenancy::{ProofFuture, Scope};
use std::sync::Arc;

pub struct Authenticator(pub Arc<UserService>);

impl Provider for Authenticator {
    fn enrolled<'a>(&'a self, scope: &'a Scope) -> ProofFuture<'a, bool> {
        Box::pin(async move {
            Ok(
                self.0.two_factor_status(scope).await?.enabled
                    || self.0.sms_enrolled(scope).await?,
            )
        })
    }
    fn methods<'a>(&'a self, scope: &'a Scope) -> ProofFuture<'a, AvailableMethods> {
        Box::pin(async move {
            let authenticator = self.0.two_factor_status(scope).await?.enabled;
            let sms = self.0.sms_factor_status(scope).await?;
            Ok(AvailableMethods {
                authenticator,
                sms: sms.company_enabled && sms.phone_verified,
                masked_destination: sms.masked_phone,
            })
        })
    }
    fn request_sms<'a>(
        &'a self,
        scope: &'a Scope,
        purpose: Purpose<'a>,
    ) -> ProofFuture<'a, SmsRequest> {
        Box::pin(async move {
            match self
                .0
                .send_step_up_sms(scope, purpose.operation, purpose.target)
                .await
            {
                Ok(sent) => Ok(SmsRequest {
                    sent: true,
                    reason: "sent",
                    masked_destination: Some(sent.masked_phone),
                    retry_after_seconds: sent.retry_after_seconds,
                }),
                Err(TwoFactorError::ResendThrottled { seconds }) => Ok(SmsRequest {
                    sent: false,
                    reason: "resend_throttled",
                    masked_destination: self.0.sms_factor_status(scope).await?.masked_phone,
                    retry_after_seconds: seconds,
                }),
                Err(TwoFactorError::HourlyLimit) => Ok(SmsRequest {
                    sent: false,
                    reason: "hourly_limit",
                    masked_destination: self.0.sms_factor_status(scope).await?.masked_phone,
                    retry_after_seconds: 3600,
                }),
                Err(TwoFactorError::SmsDisabled) => Ok(SmsRequest {
                    sent: false,
                    reason: "sms_disabled",
                    masked_destination: None,
                    retry_after_seconds: 0,
                }),
                Err(TwoFactorError::PhoneMissing | TwoFactorError::PhoneUnverified) => {
                    Ok(SmsRequest {
                        sent: false,
                        reason: "sms_not_enrolled",
                        masked_destination: None,
                        retry_after_seconds: 0,
                    })
                }
                Err(TwoFactorError::SmsUnavailable) => Ok(SmsRequest {
                    sent: false,
                    reason: "sms_unavailable",
                    masked_destination: self.0.sms_factor_status(scope).await?.masked_phone,
                    retry_after_seconds: 0,
                }),
                Err(TwoFactorError::Other(error)) => Err(error),
                Err(_) => Ok(SmsRequest {
                    sent: false,
                    reason: "rejected",
                    masked_destination: None,
                    retry_after_seconds: 0,
                }),
            }
        })
    }
    fn session<'a>(
        &'a self,
        scope: &'a Scope,
        proof: SessionProof,
    ) -> ProofFuture<'a, WindowState> {
        Box::pin(async move {
            let client = self.0.db.get().await?;
            Ok(match proof {
                SessionProof::Window {
                    window_seconds,
                    cap_seconds,
                    extend,
                } => {
                    let window = self
                        .0
                        .sessions
                        .step_up_window(&**client, scope, window_seconds, cap_seconds, extend)
                        .await?;
                    WindowState::from_flags(window.checked, window.within_cap, window.within_window)
                }
                SessionProof::Fresh { seconds } => {
                    if self
                        .0
                        .sessions
                        .spend_fresh_step_up(&**client, scope, seconds)
                        .await?
                    {
                        WindowState::Holds
                    } else {
                        WindowState::None
                    }
                }
            })
        })
    }
    fn spend<'a>(
        &'a self,
        scope: &'a Scope,
        code: &'a str,
        purpose: Purpose<'a>,
    ) -> ProofFuture<'a, Spent> {
        Box::pin(async move {
            let presented = scope.step_up_code();
            let sms_target = presented
                .and_then(|code| code.target())
                .unwrap_or(purpose.target);
            if presented.and_then(|code| code.method()) == Some("sms") {
                return match self
                    .0
                    .verify_step_up_sms(scope, code, purpose.operation, sms_target, false)
                    .await
                {
                    Ok(true) => Ok(Spent::Accepted),
                    Ok(false) | Err(TwoFactorError::InvalidCode) => Ok(Spent::Rejected),
                    Err(TwoFactorError::SmsLocked) => Ok(Spent::Locked),
                    Err(TwoFactorError::PhoneMissing | TwoFactorError::PhoneUnverified) => {
                        Ok(Spent::NotEnrolled)
                    }
                    Err(TwoFactorError::Other(error)) => Err(error),
                    Err(_) => Ok(Spent::Rejected),
                };
            }
            let spend = Spend::Purpose {
                operation: purpose.operation.to_owned(),
                channel: purpose.channel.as_str(),
            };
            let outcome = match self.0.prove(scope, code).await {
                Ok(step) => self.0.consume(scope, step, spend).await,
                Err(error) => Err(error),
            };
            match outcome {
                Ok(()) => Ok(Spent::Accepted),
                Err(TwoFactorError::Locked { .. }) => Ok(Spent::Locked),
                Err(TwoFactorError::InvalidCode | TwoFactorError::NotEnabled) => {
                    match self
                        .0
                        .verify_step_up_sms(scope, code, purpose.operation, sms_target, false)
                        .await
                    {
                        Ok(true) => Ok(Spent::Accepted),
                        Ok(false) | Err(TwoFactorError::InvalidCode) => {
                            if self.0.sms_enrolled(scope).await? {
                                Ok(Spent::Rejected)
                            } else {
                                Ok(Spent::NotEnrolled)
                            }
                        }
                        Err(TwoFactorError::SmsLocked) => Ok(Spent::Locked),
                        Err(TwoFactorError::Other(error)) => Err(error),
                        Err(_) => Ok(Spent::Rejected),
                    }
                }
                Err(TwoFactorError::Other(error)) => Err(error),
                Err(_) => Ok(Spent::Rejected),
            }
        })
    }
    fn prompt<'a>(&'a self, scope: &'a Scope, back: &'a str) -> ProofFuture<'a, String> {
        Box::pin(async move { self.0.step_up_destination(scope, back).await })
    }
}
