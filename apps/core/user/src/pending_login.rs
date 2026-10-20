//! Pending sign-in isolation for the optional login step-up. No Bilimbi module mirrors this file.
use crate::{identity, CredentialError, Login, TwoFactorError, UserService, LOGIN_STEP_UP};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bilibi_database::Error;
use bilibi_security::{guard, Scope};
use sha2::{Digest, Sha256};

const LOGIN_TARGET: &str = "pending-login";

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum LoginMethod {
    Authenticator,
    Sms,
}

/// The anonymous browser receives only `handle`; the session token remains server-side.
pub struct PendingLoginState {
    pub handle: String,
    pub authenticator_available: bool,
    pub sms_available: bool,
    pub masked_destination: Option<String>,
    pub sms_sent: bool,
}

pub enum LoginOutcome {
    Ready(Login),
    StepUp(PendingLoginState),
}

#[derive(Debug, thiserror::Error)]
pub enum PendingLoginError {
    #[error("This sign-in challenge is invalid or has expired.")]
    Invalid,
    #[error(transparent)]
    Proof(#[from] TwoFactorError),
    #[error(transparent)]
    Other(#[from] Error),
}

fn handle_hash(handle: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(handle.as_bytes()))
}

fn fresh_handle() -> Result<String, Error> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Internal)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

impl UserService {
    pub async fn begin_login(
        &self,
        email: &str,
        password: &str,
    ) -> std::result::Result<LoginOutcome, CredentialError> {
        let login = identity::login(&self.db, email, password).await?;
        let client = self.db.get().await.map_err(Error::from)?;
        let scope = match guard(&**client, &login.token, &self.sessions).await {
            Ok(scope) => scope,
            Err(error) => {
                client
                    .execute("DELETE FROM sessions WHERE id=$1", &[&login.token])
                    .await?;
                return Err(error.into());
            }
        };
        match self
            .step_up
            .require(&scope, LOGIN_STEP_UP, LOGIN_TARGET)
            .await
        {
            Ok(()) => Ok(LoginOutcome::Ready(login)),
            Err(Error::StepUpRequired { .. }) => {
                let authenticator_available = self.two_factor_status(&scope).await?.enabled;
                let sms = self.sms_factor_status(&scope).await?;
                let handle = fresh_handle()?;
                let hash = handle_hash(&handle);
                if let Err(error) = client
                    .execute(
                        "INSERT INTO user_pending_logins(handle_hash,session_id,expires_at) VALUES($1,$2,now()+interval '5 minutes')",
                        &[&hash, &login.token],
                    )
                    .await
                {
                    let _ = client.execute("DELETE FROM sessions WHERE id=$1", &[&login.token]).await;
                    return Err(Error::from(error).into());
                }
                Ok(LoginOutcome::StepUp(PendingLoginState {
                    handle,
                    authenticator_available,
                    sms_available: sms.company_enabled && sms.phone_verified,
                    masked_destination: sms.masked_phone,
                    sms_sent: false,
                }))
            }
            Err(error) => {
                client
                    .execute("DELETE FROM sessions WHERE id=$1", &[&login.token])
                    .await?;
                Err(error.into())
            }
        }
    }

    pub async fn pending_login(
        &self,
        handle: &str,
    ) -> std::result::Result<PendingLoginState, PendingLoginError> {
        let (_, scope) = self.resolve_pending_login(handle).await?;
        let authenticator_available = self.two_factor_status(&scope).await?.enabled;
        let sms = self.sms_factor_status(&scope).await?;
        let client = self.db.get().await.map_err(Error::from)?;
        let sms_sent = client
            .query_opt(
                "SELECT 1 FROM user_sms_challenges WHERE tenant_id=$1 AND user_id=$2 AND purpose=$3 AND target=$4 AND status='sent' AND expires_at>now() ORDER BY created_at DESC LIMIT 1",
                &[&scope.tenant(), &scope.user(), &LOGIN_STEP_UP, &LOGIN_TARGET],
            )
            .await
            .map_err(Error::from)?
            .is_some();
        Ok(PendingLoginState {
            handle: handle.to_owned(),
            authenticator_available,
            sms_available: sms.company_enabled && sms.phone_verified,
            masked_destination: sms.masked_phone,
            sms_sent,
        })
    }

    pub async fn send_pending_login_sms(
        &self,
        handle: &str,
    ) -> std::result::Result<crate::SmsChallengeSent, PendingLoginError> {
        let (_, scope) = self.resolve_pending_login(handle).await?;
        Ok(self
            .send_step_up_sms(&scope, LOGIN_STEP_UP, LOGIN_TARGET)
            .await?)
    }

    pub async fn complete_pending_login(
        &self,
        handle: &str,
        method: LoginMethod,
        code: &str,
    ) -> std::result::Result<Login, PendingLoginError> {
        let (token, scope) = self.resolve_pending_login(handle).await?;
        match method {
            LoginMethod::Authenticator => self.step_up(&scope, code).await?,
            LoginMethod::Sms => {
                if !self
                    .verify_step_up_sms(&scope, code, LOGIN_STEP_UP, LOGIN_TARGET, true)
                    .await?
                {
                    return Err(TwoFactorError::InvalidCode.into());
                }
            }
        }
        self.step_up
            .require(&scope, LOGIN_STEP_UP, LOGIN_TARGET)
            .await?;
        let client = self.db.get().await.map_err(Error::from)?;
        let removed = client
            .execute(
                "DELETE FROM user_pending_logins WHERE handle_hash=$1 AND session_id=$2 AND expires_at>now()",
                &[&handle_hash(handle), &token],
            )
            .await
            .map_err(Error::from)?;
        if removed != 1 {
            return Err(PendingLoginError::Invalid);
        }
        Ok(Login {
            token,
            csrf: scope.csrf,
        })
    }

    pub async fn cancel_pending_login(&self, handle: &str) -> Result<(), Error> {
        let client = self.db.get().await.map_err(Error::from)?;
        client
            .execute(
                "WITH pending AS (DELETE FROM user_pending_logins WHERE handle_hash=$1 RETURNING session_id) DELETE FROM sessions s USING pending p WHERE s.id=p.session_id",
                &[&handle_hash(handle)],
            )
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    async fn resolve_pending_login(
        &self,
        handle: &str,
    ) -> std::result::Result<(String, Scope), PendingLoginError> {
        if handle.len() < 32 || handle.len() > 128 {
            return Err(PendingLoginError::Invalid);
        }
        let hash = handle_hash(handle);
        let client = self.db.get().await.map_err(Error::from)?;
        let row = client
            .query_opt(
                "SELECT session_id,expires_at>now() FROM user_pending_logins WHERE handle_hash=$1",
                &[&hash],
            )
            .await
            .map_err(Error::from)?;
        let Some(row) = row else {
            return Err(PendingLoginError::Invalid);
        };
        let token: String = row.get(0);
        if !row.get::<_, bool>(1) {
            client
                .execute(
                    "WITH pending AS (DELETE FROM user_pending_logins WHERE handle_hash=$1 RETURNING session_id) DELETE FROM sessions s USING pending p WHERE s.id=p.session_id",
                    &[&hash],
                )
                .await
                .map_err(Error::from)?;
            return Err(PendingLoginError::Invalid);
        }
        let scope = guard(&**client, &token, &self.sessions)
            .await
            .map_err(|_| PendingLoginError::Invalid)?;
        Ok((token, scope))
    }
}
