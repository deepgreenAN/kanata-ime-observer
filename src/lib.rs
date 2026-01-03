pub mod args;
mod error;
pub mod kanata_tcp_types;

#[cfg(all(feature = "fcitx", target_os = "linux"))]
pub mod fcitx;

#[cfg(all(not(feature = "fcitx"), target_os = "linux"))]
pub mod ibus;

#[cfg(all(feature = "winonoff", target_os = "windows"))]
pub mod win_onoff;

#[cfg(all(not(feature = "winonoff"), target_os = "windows"))]
pub mod win;

pub use error::AppError;

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};

use log::{debug, error};
use once_cell::sync::OnceCell;

pub static FATAL_ERROR_SENDER: OnceCell<SyncSender<AppError>> = OnceCell::new();
pub static FATAL_ERROR_RECEIVER: OnceCell<Mutex<Receiver<AppError>>> = OnceCell::new();

pub struct FatalError(Arc<OnceCell<AppError>>);

impl FatalError {
    pub fn new() -> FatalError {
        Self(Arc::new(OnceCell::new()))
    }
    pub fn is_none(&self) -> bool {
        self.0.get().is_none()
    }
}

impl Default for FatalError {
    fn default() -> Self {
        Self(Arc::new(OnceCell::new()))
    }
}

impl Clone for FatalError {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

/// catch_fatal_errorされているFatalErrorにグローバルセンダーを通してエラーを送信する．
pub fn send_fatal_error(err: AppError) {
    if let Some(sender) = FATAL_ERROR_SENDER.get() {
        if let Err(e) = sender.try_send(err) {
            match e {
                TrySendError::Disconnected(_) => {
                    error!("Inner bug. FATAL_ERROR_SENDER was disconnected. : {e}")
                }
                TrySendError::Full(_) => debug!("FATAL_ERROR_SENDER is full. :{e}"),
            }
        }
    } else {
        error!("Inner bug. Set the FATAL_ERROR_SENDER");
    }
}

/// 内部Senderのエラーハンドリング
pub fn handle_try_send<T>(sender: &SyncSender<T>, value: T, sender_name: String) {
    if let Err(e) = sender.try_send(value) {
        match e {
            TrySendError::Disconnected(_) => {
                send_fatal_error(AppError::InnerSenderError { sender_name });
            }
            TrySendError::Full(_) => log::debug!("{e}"),
        }
    }
}

/// fatal_errorの初期化ごとに呼ぶ．ブロッキングするため必ず別スレッドにする．
pub fn catch_fatal_error(fatal_error: FatalError) {
    if let Some(receiver) = FATAL_ERROR_RECEIVER.get() {
        match receiver.try_lock() {
            Ok(guard) => {
                if let Ok(err) = guard.recv() {
                    error!("{err}");
                    let _ = fatal_error.0.set(err);
                }
            }
            Err(e) => {
                error!(
                    "Inner bug. fatal_error_loop (FATAL_ERROR_RECEIVER) must be used in a single thread at the same time.: {e}"
                );
            }
        }
    } else {
        error!("Inner bug. Set the FATAL_ERROR_RECEIVER");
    }
}

/// アプリケーションの最初に呼ぶ．
pub fn initialize_fatal_error_channel() {
    let (sender, receiver) = sync_channel(1);

    let _ = FATAL_ERROR_RECEIVER.set(Mutex::new(receiver));
    let _ = FATAL_ERROR_SENDER.set(sender);
}

#[derive(Debug)]
pub enum Command {
    Config(HashMap<String, usize>),
    Layer(HashMap<String, String>),
    Log,
}
