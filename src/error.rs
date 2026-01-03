#[allow(clippy::enum_variant_names)]
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// ループを終了し，再接続を目指す．
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    ZbusError(#[from] zbus::Error),

    /// 起こりうるためログのみとする．
    #[cfg(target_os = "linux")]
    #[error("DbusParseError")]
    DbusParseError,

    /// UIループの場合各スレッドのループを終了する。その他では失敗しやすいため、ログのみとする．
    #[cfg(target_os = "windows")]
    #[error("WinApiError")]
    WinApiError(String),

    /// 各スレッドのループを終了する．
    #[error("InnerReceiverError: inner sender was dropped. Receiver: {receiver_name}")]
    InnerReceiverError { receiver_name: String },

    /// 各スレッドのループを終了する．
    #[error("InnerSenderError: inner receiver was dropped. Sender: {sender_name}")]
    InnerSenderError { sender_name: String },

    /// スレッドのループを終了する．
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error("CustomError: {0}")]
    CustomError(String),

    #[error("ArgError: {0}")]
    ArgError(String),

    #[error("KanataMessageError")]
    KanataMessageError,
}

impl From<lexopt::Error> for AppError {
    fn from(value: lexopt::Error) -> Self {
        AppError::ArgError(value.to_string())
    }
}

#[cfg(target_os = "windows")]
impl From<windows::core::Error> for AppError {
    fn from(value: windows::core::Error) -> Self {
        AppError::WinApiError(value.to_string())
    }
}
