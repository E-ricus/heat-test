use std::{fs, path::Path, time::Duration};

use anyhow::Result;
use chrono::Utc;
use tokio::{sync::mpsc::Sender, time};

use crate::{DeviceConfig, Message};

#[derive(Debug, Clone)]
pub struct DeviceReader {
    current_value: f64,
    name: String,
    sender: Sender<Message>,
    file_path: String,
    cycle_time: Duration,
}

impl DeviceReader {
    pub(crate) fn from_config(
        name: String,
        config: DeviceConfig,
        sender: Sender<Message>,
    ) -> Result<Self> {
        let p = Path::new(&config.file);
        let current_value = fs::read_to_string(p)?.trim().parse()?;
        let cycle_time = config.cycle_time_ms.parse().map(Duration::from_millis)?;
        Ok(Self {
            name,
            current_value,
            sender,
            cycle_time,
            file_path: config.file,
        })
    }

    pub(crate) async fn read(&mut self) -> Result<()> {
        let mut interval = time::interval(self.cycle_time);
        loop {
            interval.tick().await;
            self.read_content().await?;
        }
    }

    pub(crate) async fn send_current_value(&self) -> Result<()> {
        Ok(self
            .sender
            .send(Message::ValueChange((
                self.name.clone(),
                self.current_value,
            )))
            .await?)
    }

    async fn read_content(&mut self) -> Result<()> {
        let p = Path::new(&self.file_path);
        let new_value = fs::read_to_string(p)?.trim().parse()?;
        if new_value != self.current_value {
            self.current_value = new_value;
            self.send_current_value().await?;
        }
        let now = Utc::now();
        let timestamp = now.to_rfc3339();
        println!("{timestamp:?} {}: {:.2}", self.name, self.current_value);
        Ok(())
    }
}
