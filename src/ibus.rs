use crate::{AppError, FatalError, handle_try_send, send_fatal_error};

use log::{debug, error, info};
use zbus::{
    Error as ZbusError,
    blocking::connection::Builder,
    proxy,
    zvariant::{OwnedStructure, Str, Value},
};

use std::{
    process::Command,
    sync::mpsc::{Receiver, sync_channel},
    time::Duration,
};

#[proxy(
    default_service = "org.freedesktop.IBus",
    default_path = "/org/freedesktop/IBus",
    interface = "org.freedesktop.IBus"
)]
trait Ibus {
    fn get_global_engine(&self) -> Result<OwnedStructure, ZbusError>;
}

fn body_to_engine_name(body: OwnedStructure) -> Result<String, AppError> {
    if let Value::Value(inner) = body
        .0
        .into_fields()
        .into_iter()
        .next()
        .ok_or(AppError::DbusParseError)?
        && let Value::Structure(inner_structure) = *inner
        && let Ok(engine_name) = Str::try_from(
            inner_structure
                .fields()
                .get(2)
                .ok_or(AppError::DbusParseError)?,
        )
    {
        Ok(engine_name.as_str().to_string())
    } else {
        Err(AppError::DbusParseError)
    }
}

#[derive(Debug)]
pub struct IbusImeReceiverConfig {
    pub polling_span: u64,
}

impl Default for IbusImeReceiverConfig {
    fn default() -> Self {
        Self { polling_span: 100 }
    }
}

pub struct IbusImeReceiver {
    _worker_handle: std::thread::JoinHandle<()>,
    inner_receiver: Receiver<String>,
    pre_ime_status: Option<String>,
}

impl IbusImeReceiver {
    pub fn new(config: &IbusImeReceiverConfig, fatal_error: &FatalError) -> Result<Self, AppError> {
        let IbusImeReceiverConfig { polling_span } = config;

        let cmd_out = Command::new("ibus")
            .arg("address")
            .output()
            .map_err(|_| AppError::CustomError("Cannot get 'ibus address'".to_string()))?;

        let mut address = String::from_utf8(cmd_out.stdout)
            .map_err(|_| AppError::CustomError("Cannot get 'ibus address'".to_string()))?;
        address = address.trim_end().to_string();

        let conn = Builder::address(address.as_str())?.build()?;
        info!("Connected to address: '{}'", address);

        let proxy = IbusProxyBlocking::new(&conn)?;
        let (inner_sender, inner_receiver) = sync_channel(1);

        let _worker_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();
            let polling_span = *polling_span;
            move || {
                while fatal_error.is_none() {
                    std::thread::sleep(Duration::from_millis(polling_span));

                    match proxy.get_global_engine() {
                        Ok(body) => match body_to_engine_name(body) {
                            Ok(ime_engine) => {
                                handle_try_send(
                                    &inner_sender,
                                    ime_engine,
                                    "IbusImeReceiver inner receiver".to_string(),
                                );
                            }
                            Err(e) => {
                                error!("{e}");
                            } // dbusメソッドコールのパースの失敗
                        },
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
                        receiver_name: "IbusImeReceiver inner receiver".to_string(),
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
    pub fn shutdown(self) {
        send_fatal_error(AppError::CustomError("Receiver shutdown.".to_string()));
        let _ = self._worker_handle.join();
        debug!("IbusImeReceiver shutdown.")
    }
}
