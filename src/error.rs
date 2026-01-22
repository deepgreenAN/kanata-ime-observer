/// アプリケーションのエラー。以下の方針に従う。
/// - TCP接続のエラーは再接続を試みる。
/// - ループ開始前にリソース取得などの際にエラーが起きた場合はアプリを終了する。
/// - ループ中に起きた場合、無視できるものはログ、重大なものはFatalErrorを投げて再接続を試みる。
#[allow(clippy::enum_variant_names)]
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Dbusの汎用的なエラー。
    #[cfg(target_os = "linux")]
    #[error("DbusError: {0}")]
    DbusError(String),

    /// Dbusデータのパースエラー。
    #[cfg(target_os = "linux")]
    #[error("DbusParseError: {0}")]
    DbusParseError(String),

    /// WindowsAPIに関するエラー。
    #[cfg(target_os = "windows")]
    #[error("WinApiError: {0}")]
    WinApiError(String),

    /// MacAPIに関するエラー。
    #[cfg(target_os = "macos")]
    #[error("MacApiError: {0}")]
    MacApiError(String),

    /// 内部レシーバーのエラー。相手センダーがドロップした際に起こる。基本的にFatalErrorを投げる。
    #[error("InnerReceiverError: inner sender was dropped. Receiver: {receiver_name}")]
    InnerReceiverError { receiver_name: String },

    /// 内部センダーのエラー。相手レシーバーがドロップした際に起こる。基本的にFatalErrorを投げる。
    #[error("InnerSenderError: inner receiver was dropped. Sender: {sender_name}")]
    InnerSenderError { sender_name: String },

    /// kanataとの通信の際のSerdeエラー。
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),

    /// kanataとの通信の際のIOエラー。
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    /// その他のエラー。
    #[error("CustomError: {0}")]
    CustomError(String),

    /// コマンドライン引数に関するエラー。
    #[error("ArgError: {0}")]
    ArgError(String),

    /// 未知のkanataメッセージに関するエラー。
    #[error("KanataMessageError")]
    KanataMessageError,

    /// FatalErrorを検知してループを終了するエラー。基本的にログが残りループが終了し、再接続を試みる。
    #[error("{location} was stopped by fatal error.")]
    CaughtFatalError { location: String },
}

impl From<lexopt::Error> for AppError {
    fn from(value: lexopt::Error) -> Self {
        AppError::ArgError(value.to_string())
    }
}

#[cfg(target_os = "linux")]
impl From<dbus::Error> for AppError {
    fn from(value: dbus::Error) -> Self {
        let name = value
            .name()
            .map(|name| name.to_owned())
            .unwrap_or("UnknownDbusError".to_string());
        let message = value
            .message()
            .map(|message| message.to_owned())
            .unwrap_or("unknown dbus message".to_string());

        AppError::DbusError(format!("{name}:{message}"))
    }
}

#[cfg(target_os = "windows")]
impl From<windows::core::Error> for AppError {
    fn from(value: windows::core::Error) -> Self {
        AppError::WinApiError(value.to_string())
    }
}
