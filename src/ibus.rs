use crate::{AppError, FatalError, Message, MessageReceiver, send_fatal_error, send_message};

use dbus::{blocking::SyncConnection, channel::Channel, message::MatchRule};
use log::{debug, error, info};

use std::{process::Command, time::Duration};

#[derive(Debug)]
pub struct IbusImeReceiverConfig {}

#[allow(clippy::derivable_impls)]
impl Default for IbusImeReceiverConfig {
    fn default() -> Self {
        Self {}
    }
}

pub struct IbusImeReceiver {
    _worker_handle: std::thread::JoinHandle<()>,
    message_receiver: MessageReceiver,
    pre_ime_status: Option<String>,
}

impl IbusImeReceiver {
    pub fn new(
        message_receiver: MessageReceiver,
        config: &IbusImeReceiverConfig,
        fatal_error: &FatalError,
    ) -> Result<Self, AppError> {
        let IbusImeReceiverConfig {} = config;

        let cmd_out = Command::new("ibus")
            .arg("address")
            .output()
            .map_err(|_| AppError::CustomError("Cannot get 'ibus address'".to_string()))?;

        let mut address = String::from_utf8(cmd_out.stdout)
            .map_err(|_| AppError::CustomError("Cannot get 'ibus address'".to_string()))?;
        address = address.trim_end().to_string();

        let conn: SyncConnection = Channel::open_private(&address)?.into();
        info!("Connected to address: '{}'", address);

        let proxy = conn.with_proxy(
            "org.freedesktop.IBus",
            "/org/freedesktop/IBus",
            std::time::Duration::from_millis(500),
        );

        let signal_rule = MatchRule::new_signal("org.freedesktop.IBus", "GlobalEngineChanged");

        let id = proxy.match_start(
            signal_rule,
            true,
            Box::new(|message, _| {
                match message.read1::<String>() {
                    Ok(engine_name) => {
                        send_message(Message::ImeStatus(engine_name));
                    }
                    Err(_) => {
                        error!("{}", AppError::DbusParseError);
                    }
                }
                true
            }),
        )?;

        let _worker_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();

            move || {
                while fatal_error.is_none() {
                    if let Err(e) = conn.process(Duration::from_millis(1000)) {
                        send_fatal_error(e.into());
                    }
                }

                if let Err(e) = conn.remove_match(id) {
                    error!("{}", Into::<AppError>::into(e));
                }
            }
        });

        Ok(Self {
            _worker_handle,
            message_receiver,
            pre_ime_status: None,
        })
    }
    pub fn receive(&mut self) -> Result<String, AppError> {
        loop {
            if let Message::ImeStatus(new_ime_status) =
                self.message_receiver
                    .recv()
                    .map_err(|_| AppError::InnerReceiverError {
                        receiver_name: "IbusImeReceiver message_receiver".to_string(),
                    })?
            {
                if let Some(pre_ime_status) = &self.pre_ime_status {
                    if *pre_ime_status != new_ime_status {
                        self.pre_ime_status = Some(new_ime_status.clone());
                        return Ok(new_ime_status);
                    }
                } else {
                    self.pre_ime_status = Some(new_ime_status.clone());
                    return Ok(new_ime_status);
                }
            }
        }
    }
    pub fn shutdown(self) -> MessageReceiver {
        send_fatal_error(AppError::CustomError("Receiver shutdown.".to_string()));
        self._worker_handle.join().expect("worker thread panicked.");
        debug!("IbusImeReceiver shutdown.");

        self.message_receiver
    }
}
