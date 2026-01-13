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

#[cfg(target_os = "macos")]
pub mod mac;

pub use error::AppError;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use log::{debug, error};
use once_cell::sync::OnceCell;

/// 致命的なエラーを検知するためのレシーバー・センダー。
pub type FatalErrorReceiver = Receiver<AppError>;
pub type FatalErrorSender = SyncSender<AppError>;

/// 致命的なエラーを検知するためのグローバルセンダー。
pub static FATAL_ERROR_SENDER: OnceCell<FatalErrorSender> = OnceCell::new();

/// ime情報を取得するタイミングを通知するためのメッセージ。
pub enum Message {
    GetImeStatus,
    CaughtFatalError,
}

/// ime情報を取得するタイミングを通知するためのメッセージのレシーバー
pub type MessageReceiver = Receiver<Message>;
pub type MessageSender = SyncSender<Message>;

/// ime情報を取得するタイミングを通知するためのメッセージのグローバルセンダー。
pub static MESSAGE_SENDER: OnceCell<MessageSender> = OnceCell::new();

/// アプリケーションの最初に呼ぶ．
pub fn initialize_app() -> Result<(MessageReceiver, FatalErrorReceiver), AppError> {
    let (get_ime_status_message_sender, get_ime_status_message_receiver) = sync_channel(1);
    MESSAGE_SENDER
        .set(get_ime_status_message_sender)
        .map_err(|_| AppError::CustomError("Inner bug. initialized multiple times.".to_string()))?;

    let (fatal_error_sender, fatal_error_receiver) = sync_channel(1);
    FATAL_ERROR_SENDER
        .set(fatal_error_sender)
        .map_err(|_| AppError::CustomError("Inner bug. initialized multiple times.".to_string()))?;

    Ok((get_ime_status_message_receiver, fatal_error_receiver))
}

/// ime情報を受け渡すためのレシーバー
pub type InnerReceiver = Receiver<String>;

/// 致命的なエラー。全てのスレッドを終了し再接続を試みる。
pub struct FatalError(Arc<OnceCell<AppError>>);

impl FatalError {
    fn new() -> FatalError {
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
pub fn catch_fatal_error(fatal_error: FatalError, fatal_error_receiver: &FatalErrorReceiver) {
    if let Ok(app_err) = fatal_error_receiver.recv() {
        let err_str = app_err.to_string();
        if fatal_error.0.set(app_err).is_ok() {
            error!("Caught the FatalError: {err_str}");
        }
    }
}

/// グローバルレシーバーの初期化・FatalErrorの作成．ループの最初に呼ぶ．
pub fn initialize_fatal_error(fatal_error_receiver: &FatalErrorReceiver) -> FatalError {
    // レシーバー内にデータがある場合は取り出す．
    if let Ok(unused_fatal_error) = fatal_error_receiver.try_recv() {
        debug!("Unused fatal error discarded: {unused_fatal_error}");
    }

    FatalError::new()
}

pub fn send_message(message: Message) {
    if let Some(message_sender) = MESSAGE_SENDER.get() {
        handle_try_send(message_sender, message, "MESSAGE_SENDER".to_string());
    } else {
        send_fatal_error(AppError::CustomError(
            "GET_IME_STATUS_MESSAGE_SENDER".to_owned(),
        ));
    }
}

/// cli用のコマンド。
#[derive(Debug)]
pub enum Command {
    Config(HashMap<String, usize>),
    Layer(HashMap<String, String>),
    Log,
}
