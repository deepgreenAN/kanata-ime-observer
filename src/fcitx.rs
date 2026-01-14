use crate::{
    AppError, FatalError, InnerReceiver, Message, MessageReceiver, handle_try_send,
    send_fatal_error, send_message,
};

use log::{debug, error, info};
use zbus::{Error as ZbusError, blocking::Connection, proxy};

use std::{sync::mpsc::sync_channel, time::Duration};

#[proxy(
    default_service = "org.fcitx.Fcitx5",
    default_path = "/controller",
    interface = "org.fcitx.Fcitx.Controller1"
)]
trait Controller {
    fn current_input_method(&self) -> Result<String, ZbusError>;
}

#[derive(Debug)]
pub struct FcitxImeReceiverConfig {
    pub polling_span: u64,
}

impl Default for FcitxImeReceiverConfig {
    fn default() -> Self {
        Self { polling_span: 100 }
    }
}
pub struct FcitxImeReceiver {
    _worker_handle: std::thread::JoinHandle<MessageReceiver>,
    _polling_handle: std::thread::JoinHandle<()>,
    inner_receiver: InnerReceiver,
    pre_ime_status: Option<String>,
}

impl FcitxImeReceiver {
    pub fn new(
        message_receiver: MessageReceiver,
        config: &FcitxImeReceiverConfig,
        fatal_error: &FatalError,
    ) -> Result<Self, AppError> {
        let FcitxImeReceiverConfig { polling_span } = config;

        let conn = Connection::session()?;
        info!("Connected to 'org.fcitx.Fcitx5'");

        let proxy = ControllerProxyBlocking::new(&conn)?;
        let (inner_sender, inner_receiver) = sync_channel(1);

        let _worker_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();

            move || {
                while let Ok(_msg) = message_receiver.recv()
                    && fatal_error.is_none()
                {
                    match proxy.current_input_method() {
                        Ok(ime_engine) => {
                            handle_try_send(
                                &inner_sender,
                                ime_engine,
                                "FcitxImeReceiver inner sender".to_string(),
                            );
                        }
                        Err(zbus_err) => match zbus_err {
                            ZbusError::InputOutput(_)
                            | ZbusError::MethodError(_, _, _)
                            | ZbusError::InvalidGUID => {
                                send_fatal_error(zbus_err.into());
                            }
                            _ => {
                                error!("{zbus_err}");
                            }
                        },
                    }
                }

                message_receiver
            }
        });

        let _polling_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();
            let polling_span = *polling_span;

            move || {
                while fatal_error.is_none() {
                    std::thread::sleep(Duration::from_millis(polling_span));
                    send_message(Message::GetImeStatus);
                }
            }
        });

        Ok(Self {
            _worker_handle,
            _polling_handle,
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
                        receiver_name: "FcitxImeReceiver inner receiver".to_owned(),
                    })?;

            if let Some(pre_ime_status) = &self.pre_ime_status {
                // match式に
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
        self._polling_handle
            .join()
            .expect("polling thread panicked.");
        let message_receiver = self._worker_handle.join().expect("worker thread panicked.");
        debug!("FcitxImeReceiver shutdown.");

        message_receiver
    }
}
