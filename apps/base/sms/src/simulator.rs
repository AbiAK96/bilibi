//! Development SMS inbox. No Bilimbi module mirrors this file.
use crate::{DeliveryOutcome, Error, GatewayFuture, Message, SmsGateway};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::SystemTime};

#[derive(Clone, Deserialize, Serialize)]
pub struct SimulatorEntry {
    pub id: String,
    pub to: String,
    pub body: String,
    pub sent_at_unix: u64,
}

pub struct Simulator {
    directory: PathBuf,
    keep: usize,
}
impl Simulator {
    pub fn new(directory: PathBuf, keep: usize) -> Self {
        Self {
            directory,
            keep: keep.max(1),
        }
    }
    pub async fn entries(&self) -> crate::Result<Vec<SimulatorEntry>> {
        let mut directory = match tokio::fs::read_dir(&self.directory).await {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(Error::Delivery),
        };
        let mut entries = Vec::new();
        while let Some(item) = directory.next_entry().await.map_err(|_| Error::Delivery)? {
            let path = item.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let bytes = tokio::fs::read(path).await.map_err(|_| Error::Delivery)?;
            if let Ok(entry) = serde_json::from_slice(&bytes) {
                entries.push(entry);
            }
        }
        entries.sort_by(|left: &SimulatorEntry, right: &SimulatorEntry| {
            right
                .sent_at_unix
                .cmp(&left.sent_at_unix)
                .then_with(|| right.id.cmp(&left.id))
        });
        entries.truncate(self.keep);
        Ok(entries)
    }
    async fn store(&self, message: &Message) -> crate::Result<String> {
        tokio::fs::create_dir_all(&self.directory)
            .await
            .map_err(|_| Error::Delivery)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&self.directory, std::fs::Permissions::from_mode(0o700))
                .await
                .map_err(|_| Error::Delivery)?;
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let entry = SimulatorEntry {
            id: id.clone(),
            to: message.to.clone(),
            body: message.body.clone(),
            sent_at_unix: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let path = self.directory.join(format!("{id}.json"));
        tokio::fs::write(
            &path,
            serde_json::to_vec(&entry).map_err(|_| Error::Delivery)?,
        )
        .await
        .map_err(|_| Error::Delivery)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .await
                .map_err(|_| Error::Delivery)?;
        }
        let entries = self.entries().await?;
        if entries.len() >= self.keep {
            let mut directory = tokio::fs::read_dir(&self.directory)
                .await
                .map_err(|_| Error::Delivery)?;
            let retained: std::collections::HashSet<_> =
                entries.into_iter().map(|entry| entry.id).collect();
            while let Some(item) = directory.next_entry().await.map_err(|_| Error::Delivery)? {
                let path = item.path();
                let stem = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default();
                if path.extension().and_then(|value| value.to_str()) == Some("json")
                    && !retained.contains(stem)
                {
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
        }
        Ok(id)
    }
}
impl SmsGateway for Simulator {
    fn deliver<'a>(&'a self, message: &'a Message) -> GatewayFuture<'a> {
        Box::pin(async move {
            let reference = self.store(message).await?;
            Ok(DeliveryOutcome {
                provider: "simulator",
                reference: Some(reference),
            })
        })
    }
    fn describe(&self) -> &'static str {
        "development simulator"
    }
}
