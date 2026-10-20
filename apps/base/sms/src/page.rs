//! Development simulator inbox page. No Bilimbi module mirrors this file.
use crate::Sms;
use askama::Template;
use bilibi_database::Error;
use bilibi_tenancy::Scope;
use bilibi_ui::{
    page::{Page, PageBody, PageFuture, PageRequest},
    SmsInboxPage, SmsInboxRow,
};
use std::sync::Arc;

pub struct Pages {
    pub sms: Arc<Sms>,
}

impl Page<Scope, Error> for Pages {
    fn render<'a>(&'a self, request: PageRequest<'a, Scope>) -> PageFuture<'a, Error> {
        Box::pin(async move {
            if request.operation != "inbox" || request.scope.is_some() {
                return Err(Error::Forbidden);
            }
            let simulator = self.sms.simulator().ok_or(Error::NotFound)?;
            let messages = simulator
                .entries()
                .await
                .map_err(|_| Error::Internal)?
                .into_iter()
                .map(|message| SmsInboxRow {
                    id: message.id,
                    to: message.to,
                    body: message.body,
                    sent_at_unix: message.sent_at_unix,
                })
                .collect();
            Ok(PageBody {
                html: SmsInboxPage { messages }
                    .render()
                    .map_err(|_| Error::Internal)?,
                live: false,
                title: Some("Development SMS inbox".into()),
            })
        })
    }
}
