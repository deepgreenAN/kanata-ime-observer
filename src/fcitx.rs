use crate::{
    AppError, FatalError, InnerReceiver, Message, MessageReceiver, handle_try_send,
    send_fatal_error, send_message,
};

use dbus::blocking::SyncConnection;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::message::MatchRule;
use log::{debug, info};

use std::{sync::mpsc::sync_channel, time::Duration};

pub fn dbus_main_loop(fatal_error: &FatalError) -> Result<(), AppError> {
    let conn = SyncConnection::new_session()?;
    info!("Connected to 'session bus.'");

    let notifier_watcher_proxy = conn.with_proxy(
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        Duration::from_millis(500),
    );

    let notifier_items: Vec<String> = notifier_watcher_proxy.get(
        "org.kde.StatusNotifierWatcher",
        "RegisteredStatusNotifierItems",
    )?;

    let fcitx5_sni_proxy = {
        let mut fcitx5_sni_proxy = None;

        for sni_name in notifier_items.into_iter() {
            let dest_and_path = sni_name.split("@").collect::<Vec<&str>>();
            let (dest, path) = (
                dest_and_path
                    .first()
                    .ok_or(AppError::DbusParseError(
                        "Invalid StatusNotifierItem name".to_string(),
                    ))?
                    .to_string(),
                dest_and_path
                    .get(1)
                    .ok_or(AppError::DbusParseError(
                        "Invalid StatusNotifierItem name".to_string(),
                    ))?
                    .to_string(),
            );

            let sni_proxy = conn.with_proxy(dest, path, Duration::from_millis(500));
            let sni_id: String = sni_proxy.get("org.kde.StatusNotifierItem", "Id")?;

            if sni_id.as_str() == "Fcitx" {
                fcitx5_sni_proxy = Some(sni_proxy);
            }
        }

        fcitx5_sni_proxy
    };

    let Some(fcitx5_sni_proxy) = fcitx5_sni_proxy else {
        return Err(AppError::DbusError(
            "Couldn't find fcitx5 StatusNotifierItem".to_string(),
        ));
    };

    let signal_ml = MatchRule::new_signal("org.kde.StatusNotifierItem", "NewIcon");

    let token = fcitx5_sni_proxy.match_start(
        signal_ml,
        true,
        Box::new(move |_message, _| {
            send_message(Message::GetImeStatus);

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
pub struct FcitxImeReceiverConfig {}

#[allow(clippy::derivable_impls)]
impl Default for FcitxImeReceiverConfig {
    fn default() -> Self {
        Self {}
    }
}
pub struct FcitxImeReceiver {
    _worker_handle: std::thread::JoinHandle<MessageReceiver>,
    inner_receiver: InnerReceiver,
    pre_ime_status: Option<String>,
}

impl FcitxImeReceiver {
    pub fn new(
        message_receiver: MessageReceiver,
        config: &FcitxImeReceiverConfig,
        fatal_error: &FatalError,
    ) -> Result<Self, AppError> {
        let FcitxImeReceiverConfig {} = config;

        let conn = SyncConnection::new_session()?;
        info!("Connected to 'session bus.'");

        let (inner_sender, inner_receiver) = sync_channel(1);

        let _worker_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();

            move || {
                let proxy = conn.with_proxy(
                    "org.fcitx.Fcitx5",
                    "/controller",
                    std::time::Duration::from_millis(500),
                );

                while let Ok(_msg) = message_receiver.recv()
                    && fatal_error.is_none()
                {
                    match proxy.method_call::<(String,), _, _, _>(
                        "org.fcitx.Fcitx.Controller1",
                        "CurrentInputMethod",
                        (),
                    ) {
                        Ok((ime_engine,)) => {
                            handle_try_send(
                                &inner_sender,
                                ime_engine,
                                "FcitxImeReceiver inner sender".to_string(),
                            );
                        }
                        Err(dbus_err) => {
                            send_fatal_error(dbus_err.into());
                        }
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
                        receiver_name: "FcitxImeReceiver inner receiver".to_owned(),
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
        debug!("FcitxImeReceiver shutdown.");

        message_receiver
    }
}
