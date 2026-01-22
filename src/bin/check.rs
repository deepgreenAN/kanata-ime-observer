use kanata_ime_observer::{catch_fatal_error, initialize_app, initialize_fatal_error};

#[cfg(all(feature = "fcitx", target_os = "linux"))]
use kanata_ime_observer::fcitx::{
    FcitxImeReceiver as Receiver, FcitxImeReceiverConfig as Config, dbus_main_loop as main_loop,
};

#[cfg(all(not(feature = "fcitx"), target_os = "linux"))]
use kanata_ime_observer::ibus::{
    IbusImeReceiver as Receiver, IbusImeReceiverConfig as Config, dbus_main_loop as main_loop,
};

#[cfg(all(feature = "winonoff", target_os = "windows"))]
use kanata_ime_observer::win_onoff::{
    WindowsImeOnOffReceiver as Receiver, WindowsImeOnOffReceiverConfig as Config,
    win_main_loop as main_loop,
};

#[cfg(all(not(feature = "winonoff"), target_os = "windows"))]
use kanata_ime_observer::win::{
    WindowsImeReceiver as Receiver, WindowsImeReceiverConfig as Config, win_main_loop as main_loop,
};

#[cfg(target_os = "macos")]
use kanata_ime_observer::mac::{MacImeReceiver as Receiver, mac_main_loop as main_loop};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (app_message_receiver, app_fatal_error_receiver) = initialize_app()?;

    let fatal_error = initialize_fatal_error(&app_fatal_error_receiver);

    std::thread::spawn({
        let fatal_error = fatal_error.clone();
        move || {
            catch_fatal_error(fatal_error, &app_fatal_error_receiver);

            app_fatal_error_receiver
        }
    });

    let app_config = Config::default();

    let mut ime_receiver = Receiver::new(app_message_receiver, &app_config, &fatal_error)?;

    std::thread::spawn({
        let fatal_error = fatal_error.clone();

        move || {
            while let Ok(ime_status) = ime_receiver.receive()
                && fatal_error.is_none()
            {
                println!("ime_status: {ime_status}");
            }
        }
    });

    let _ = main_loop(&fatal_error);

    Ok(())
}
