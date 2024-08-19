mod reader;

use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use chrono::Utc;
use reader::DeviceReader;
use serde::Deserialize;
use tokio::{
    sync::mpsc::{self, Sender},
    task::{self, JoinHandle},
    time,
};

// It would be better to get this from an env variable.
// More in the README [1]
const CONFIG_FILE: &str = "./asset_list.json";

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
struct DeviceConfig {
    file: String,
    cycle_time_ms: String,
}

#[derive(Debug)]
enum Message {
    ValueChange((String, f64)),
    ConfigChange(Devices),
}

type Devices = HashMap<String, DeviceConfig>;

#[derive(Debug, Clone)]
pub struct Controller {
    devices: Devices,
    values: Arc<Mutex<HashMap<String, f64>>>,
}

impl Controller {
    pub fn new() -> Result<Self> {
        let config = fs::read_to_string(CONFIG_FILE)?;
        let devices: Devices = serde_json::from_str(&config)?;
        let values = HashMap::new();
        let values = Arc::new(Mutex::new(values));
        Ok(Self { devices, values })
    }

    pub async fn controll(self) {
        let (tx, mut rx) = mpsc::channel(100);

        // Spawns a task per device
        let (mut tx, mut handles) = create_tasks(self.devices.clone(), tx).await;

        // Spans a task to read device summary
        let v = Arc::clone(&self.values);
        task::spawn(async {
            print_total(v).await;
        });

        let config_tx = tx.clone();
        task::spawn(async {
            if let Err(e) = wait_config_change(self.devices, config_tx).await {
                println!("error listening to config changes: {e}");
            }
        });
        while let Some(m) = rx.recv().await {
            match m {
                Message::ValueChange((name, value)) => {
                    let mut values = self
                        .values
                        .lock()
                        // Safe to expect, the lock cannot be hold in this thread at the same time.
                        .expect("fail to get lock in controll thread");
                    values.insert(name, value);
                }
                // This is not the most optimal solution for changes in the config file.
                // Identifing the change, to either, cancell/create/send_updtate to a task would be better.
                // More in README [3]
                Message::ConfigChange(d) => {
                    println!("Config file changed");
                    // Aborts all the previous tasks
                    handles.into_iter().for_each(|h| h.abort());
                    self.values
                        .lock()
                        // Safe to expect, the lock cannot be hold in this thread at the same time.
                        .expect("fail to get lock in controll thread")
                        .clear();
                    // creates new tasks for the devices in the config files changed.
                    (tx, handles) = create_tasks(d, tx).await;
                }
            }
        }
    }
}

async fn wait_config_change(mut devices: Devices, tx: Sender<Message>) -> Result<()> {
    let p = Path::new(CONFIG_FILE);
    // Reading the file all the time will be really CPU heavy for the thread handling this task.
    // This interval aliviates a bit, but it is not the best solution.
    // More in the Readme [2]
    let mut interval = time::interval(Duration::from_millis(500));
    loop {
        interval.tick().await;
        let content = fs::read_to_string(p)?;
        let new_devices: Devices = serde_json::from_str(&content)?;
        if new_devices != devices {
            devices = new_devices.clone();
            tx.send(Message::ConfigChange(new_devices)).await?;
        }
    }
}

async fn print_total(values: Arc<Mutex<HashMap<String, f64>>>) {
    let mut interval = time::interval(Duration::from_secs(2));
    loop {
        interval.tick().await;
        // Safe to expect, the lock is only hold in this thread here.
        let values = values.lock().expect("fail to get lock in summary thread");
        let total: f64 = values.values().sum();
        // Gives the mutex lock earlier
        drop(values);
        let now = Utc::now();
        let timestamp = now.to_rfc3339();
        println!("{timestamp:?} Devices Summary: {:.2}", total);
    }
}

async fn create_tasks(
    devices: Devices,
    tx: Sender<Message>,
) -> (Sender<Message>, Vec<JoinHandle<()>>) {
    let mut handles = Vec::with_capacity(devices.len());
    for (name, config) in devices.into_iter() {
        let tx = tx.clone();
        let mut reader = match DeviceReader::from_config(name, config, tx) {
            Ok(dr) => dr,
            Err(e) => {
                println!("error at device reader init: {e}");
                continue;
            }
        };
        let handle = task::spawn(async move {
            // Sends the first value to initialize the values map.
            if let Err(e) = reader.send_current_value().await {
                println!("error sending intial value: {e}")
            }
            if let Err(e) = reader.read().await {
                println!("error reading content: {e}");
            }
        });
        handles.push(handle);
    }
    (tx, handles)
}
