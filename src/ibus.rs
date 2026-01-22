use crate::{
    AppError, FatalError, InnerReceiver, Message, MessageReceiver, handle_try_send,
    send_fatal_error, send_message,
};

use dbus::{blocking::SyncConnection, channel::Channel, message::MatchRule};
use log::{debug, error, info};

use std::{process::Command, sync::mpsc::sync_channel, time::Duration};

pub fn dbus_main_loop(fatal_error: &FatalError) -> Result<(), AppError> {
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

    let token = proxy.match_start(
        signal_rule,
        true,
        Box::new(|message, _| {
            match message.read1::<String>() {
                Ok(engine_name) => {
                    send_message(Message::ImeStatus(engine_name));
                }
                Err(_) => {
                    error!(
                        "{}",
                        AppError::DbusParseError("Couldn't read GlobalEngineChanged.".to_string())
                    );
                }
            }
            true
        }),
    )?;

    // メインループ
    while fatal_error.is_none() {
        conn.process(Duration::from_millis(1000))?;
    }

    // 不要だが一応。
    conn.remove_match(token)?;

    Err(AppError::CaughtFatalError {
        location: "dbus_main_loop".to_string(),
    })
}

#[derive(Debug)]
pub struct IbusImeReceiverConfig {}

#[allow(clippy::derivable_impls)]
impl Default for IbusImeReceiverConfig {
    fn default() -> Self {
        Self {}
    }
}

pub struct IbusImeReceiver {
    _worker_handle: std::thread::JoinHandle<MessageReceiver>,
    inner_receiver: InnerReceiver,
    pre_ime_status: Option<String>,
}

impl IbusImeReceiver {
    pub fn new(
        message_receiver: MessageReceiver,
        config: &IbusImeReceiverConfig,
        fatal_error: &FatalError,
    ) -> Result<Self, AppError> {
        let IbusImeReceiverConfig {} = config;

        let (inner_sender, inner_receiver) = sync_channel(1);

        let _worker_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();

            move || {
                while let Ok(msg) = message_receiver.recv()
                    && fatal_error.is_none()
                {
                    match msg {
                        Message::ImeStatus(ime_status) => {
                            handle_try_send(
                                &inner_sender,
                                ime_status,
                                "IbusImeReceiver inner sender".to_string(),
                            );
                        }
                        Message::CaughtFatalError => {
                            handle_try_send(
                                &inner_sender,
                                String::new(),
                                "IbusImeReceiver inner sender".to_string(),
                            ); // ループを回すため
                        }
                        Message::GetImeStatus => {} // 実際には呼ばれない
                    }
                }
                message_receiver
            }
        });

        Ok(Self {
            _worker_handle,
            inner_receiver,
            pre_ime_status: None,
        })
    }
    pub fn receive(&mut self) -> Result<String, AppError> {
        loop {
            let new_ime_status =
                self.inner_receiver
                    .recv()
                    .map_err(|_| AppError::InnerReceiverError {
                        receiver_name: "IbusImeReceiver inner receiver".to_owned(),
                    })?;

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
    pub fn shutdown(self) -> MessageReceiver {
        send_fatal_error(AppError::CustomError("Receiver shutdown.".to_string()));
        let message_receiver = self._worker_handle.join().expect("worker thread panicked.");
        debug!("IbusImeReceiver shutdown.");

        message_receiver
    }
}
