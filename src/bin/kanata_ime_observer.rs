use kanata_ime_observer::{
    AppError, Command, FatalError, Message, catch_fatal_error, initialize_app,
    initialize_fatal_error,
    kanata_tcp_types::{KanataClientMessage, KanataServerResponse},
    send_fatal_error, send_message,
};

#[cfg(all(feature = "fcitx", target_os = "linux"))]
use kanata_ime_observer::fcitx::FcitxImeReceiver as Receiver;

#[cfg(all(not(feature = "fcitx"), target_os = "linux"))]
use kanata_ime_observer::ibus::IbusImeReceiver as Receiver;

#[cfg(all(feature = "winonoff", target_os = "windows"))]
use kanata_ime_observer::win_onoff::{
    WindowsImeOnOffReceiver as Receiver, win_main_loop as main_loop,
};

#[cfg(all(not(feature = "winonoff"), target_os = "windows"))]
use kanata_ime_observer::win::WindowsImeReceiver as Receiver;

#[cfg(target_os = "macos")]
use kanata_ime_observer::mac::{MacImeReceiver as Receiver, mac_main_loop as main_loop};

use log::{debug, error, info};

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

fn write_to_kanata(
    receiver: &mut Receiver,
    command: &Command,
    mut kanata_stream: TcpStream,
    fatal_error: &FatalError,
) -> Result<(), AppError> {
    while fatal_error.is_none() {
        let ime_status = receiver.receive()?;
        info!("Change of IME status was detected. ime status: \"{ime_status}\".");

        let msg = match &command {
            Command::Config(config_map) => config_map
                .get(&ime_status)
                .map(|config_num| KanataClientMessage::ReloadNum { index: *config_num }),
            Command::Layer(layer_map) => {
                layer_map
                    .get(&ime_status)
                    .map(|layer_name| KanataClientMessage::ChangeLayer {
                        new: layer_name.to_owned(),
                    })
            }
            Command::Log => None,
        };

        if let Some(msg) = msg {
            kanata_stream.write_all(serde_json::to_string(&msg)?.as_bytes())?;
            info!("Sended the message to kanata.")
        }
    }

    Err(AppError::CustomError(
        "write_to_kanata: Caught the fatal error.".to_string(),
    ))
}

fn read_from_kanata(kanata_stream: TcpStream, fatal_error: &FatalError) -> Result<(), AppError> {
    let mut kanata_read = BufReader::new(kanata_stream);
    let mut buf = String::new();

    while fatal_error.is_none() {
        buf.clear();
        if kanata_read.read_line(&mut buf)? == 0 {
            return Err(AppError::CustomError(
                "Kanata Connection finished.".to_string(),
            ));
        }

        if let Ok(response) = serde_json::from_str::<KanataServerResponse>(&buf) {
            match response.status.as_str() {
                "Ok" => {
                    debug!("Request succeeded.");
                }
                "Error" => {
                    if let Some(msg) = response.msg {
                        debug!("Request failed.: {msg}");
                    }
                }
                _ => return Err(AppError::KanataMessageError),
            }
        } else {
            debug!("Got the message from kanata: {buf}");
        }
    }

    Err(AppError::CustomError(
        "read_from_kanata: Caught the fatal error.".to_string(),
    ))
}

fn main() -> Result<(), AppError> {
    use kanata_ime_observer::args::{Args, parse_args};

    use backon::{BlockingRetryable, ExponentialBuilder};

    let Args {
        port,
        command,
        log_level,
        app_config,
    } = parse_args()?;

    let command = Arc::new(command);

    simple_logger::init_with_level(log_level).map_err(|e| AppError::CustomError(e.to_string()))?;

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

    let (mut app_message_receiver, mut app_fatal_error_receiver) = initialize_app()?;

    loop {
        let fatal_error = initialize_fatal_error(&app_fatal_error_receiver);

        let fatal_error_loop_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();
            move || {
                catch_fatal_error(fatal_error, &app_fatal_error_receiver);

                app_fatal_error_receiver
            }
        });

        let (writer_stream, reader_stream) = || -> Result<(TcpStream, TcpStream), AppError> {
            let kanata_connection = TcpStream::connect_timeout(&addr, Duration::from_secs(30))?;

            kanata_connection.set_write_timeout(Some(Duration::from_secs(5)))?;

            let writer_stream = kanata_connection.try_clone()?;
            let reader_stream = kanata_connection;
            Ok((writer_stream, reader_stream))
        }
        .retry(
            ExponentialBuilder::default()
                .with_min_delay(Duration::from_millis(100))
                .with_max_delay(Duration::from_secs(10))
                .with_max_times(10),
        )
        .notify(|e, duration| {
            info!("Failed to connect to kanata: {e}");
            info!("Retry in {duration:?}");
        })
        .call()?; // 失敗可能性がある．

        info!("Connected to kanata.");

        let mut ime_receiver = Receiver::new(app_message_receiver, &app_config, &fatal_error)?; // 失敗可能性がある

        info!("Receiver Initialized.");

        let write_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();
            let command = Arc::clone(&command);

            move || {
                if let Err(e) =
                    write_to_kanata(&mut ime_receiver, &command, writer_stream, &fatal_error)
                {
                    error!("write_to_kanata stopped: {e}");
                    send_fatal_error(e);
                }

                ime_receiver.shutdown()
            }
        });

        let read_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();
            move || {
                if let Err(e) = read_from_kanata(reader_stream, &fatal_error) {
                    error!("read_from_kanata stopped: {e}");
                    send_fatal_error(e);
                }
            }
        });

        // 以下メインスレッドの処理
        #[cfg(any(target_os = "macos", all(target_os = "windows", feature = "winonoff")))]
        if let Err(e) = main_loop(&fatal_error) {
            error!("main_loop stopped: {e}");
            send_fatal_error(e);
        }

        app_fatal_error_receiver = fatal_error_loop_handle
            .join()
            .expect("catch_fatal_error panicked.");
        send_message(Message::CaughtFatalError); // ブロッキングしているrecvを解除する。
        app_message_receiver = write_handle.join().expect("write_to_kanata panicked.");
        read_handle.join().expect("read_from_kanata panicked.");

        std::thread::sleep(Duration::from_millis(100));
        info!("Main loop restarted.");
    }
}
