//! Validated addresses and plain-text messages. Header values are the injection
//! surface, so every field is checked here and never trusted by a transport.
use crate::{Error, Result};
use lettre::message::{header::ContentType, Mailbox as LettreMailbox};

const MAX_BODY: usize = 100_000;

#[derive(Clone, Debug)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}
impl Address {
    pub fn new(name: Option<&str>, email: &str) -> Result<Self> {
        let email = email.trim();
        let (local, domain) = email.split_once('@').ok_or(Error::Invalid)?;
        if email.len() > 254
            || local.is_empty()
            || local.len() > 64
            || domain.len() < 3
            || !domain.contains('.')
            || domain.contains('@')
            || email
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || "<>,;:\"()[]\\".contains(c))
        {
            return Err(Error::Invalid);
        }
        let name = name.map(str::trim).filter(|name| !name.is_empty());
        if name.is_some_and(|name| name.len() > 200 || name.chars().any(char::is_control)) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            name: name.map(str::to_owned),
            email: email.to_owned(),
        })
    }
    fn mailbox(&self) -> Result<LettreMailbox> {
        let address = self.email.parse().map_err(|_| Error::Invalid)?;
        Ok(LettreMailbox::new(self.name.clone(), address))
    }
}
impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.email)
    }
}

pub struct Message {
    pub to: Address,
    pub subject: String,
    pub text: String,
}
impl Message {
    pub fn render(&self, from: &Address) -> Result<lettre::Message> {
        if self.subject.is_empty()
            || self.subject.len() > 200
            || self.subject.chars().any(char::is_control)
            || self.text.len() > MAX_BODY
        {
            return Err(Error::Invalid);
        }
        lettre::Message::builder()
            .from(from.mailbox()?)
            .to(self.to.mailbox()?)
            .subject(&self.subject)
            .header(ContentType::TEXT_PLAIN)
            .body(self.text.clone())
            .map_err(|_| Error::Invalid)
    }
}
