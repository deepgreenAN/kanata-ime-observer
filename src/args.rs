use crate::{AppError, Command};

#[cfg(all(feature = "fcitx", target_os = "linux"))]
use crate::fcitx::FcitxImeReceiverConfig;

#[cfg(all(not(feature = "fcitx"), target_os = "linux"))]
use crate::ibus::IbusImeReceiverConfig;

#[cfg(all(feature = "winonoff", target_os = "windows"))]
use crate::win_onoff::WindowsImeOnOffReceiverConfig;

#[cfg(all(not(feature = "winonoff"), target_os = "windows"))]
use crate::win::WindowsImeReceiverConfig;

#[cfg(target_os = "macos")]
use crate::mac::MacImeReceiverConfig;

use std::collections::HashMap;

use lexopt::{
    Parser, ValueExt,
    prelude::{Long, Short},
};
use log::Level;

fn help_str() -> String {
    "kanata_ime_observer: monitor the IME status and request kanata to change the config file / layer. 

Commands:
    kanata_ime_observer config <PORT> [-i|--ime] <IME-NAME> [OPTIONS]                                   
        Request kanata to change the config file.

    kanata_ime_observer layer <PORT> [-i|--ime] <IME-NAME> [-l|--layer] <LAYER-NAME> [OPTIONS]          
        Request kanata to change the layer.

    kanata_ime_observer log <PORT> [OPTIONS]
        Does not any request to kanata.
".to_string()
}

fn options_str() -> String {
    "Options:
    -h|--help
        Print help

    -d|--debug
        Enable debug logging.

    --polling <MILLISECOND> (win, win_onoff only) (win default 500) (win_onoff default 1000)
        Polling span [ms] of GetKeyboardLayout(win), SendMessageTimeout(win_onoff).
    
    --without-polling (win, win_onoff only)
        Disable polling.

    --retry-number <TIMES> (win_onoff only) (default 3)
        The number of retry of SendMessageTimeout.

    --sendmessage-timeout <MILLISECOND> (win_onoff only) (default 100)
        The timeout of SendMessageTimeout.

    --retry-span <MILLISECOND> (win_onoff only) (default 100)
        The span [ms] of retry SendMessageTimeout.

    --delay <MILLISECOND> (win, win_onoff, mac only) (win default 50) (win_onoff default 50) (mac default 50)
        The delay from action to GetKeyboardLayout(win), SendMessageTimeout(win_onoff), TISCopyCurrentKeyboardInputSource(mac).  
".to_string()
}

fn config_help_str() -> String {
    format!(
        "kanata_ime_observer config: monitor the IME status and request kanata to change the config file.

Usage:
    kanata_ime_observer config <PORT> [-i|--ime] <IME-NAME> [OPTIONS]

{}",
        options_str(),
    )
}

fn layer_help_str() -> String {
    format!(
        "kanata_ime_observer layer: monitor the IME status and request kanata to change the layer.

Usage:
    kanata_ime_observer layer <PORT> [-i|--ime] <IME-NAME> [-l|--layer] <LAYER-NAME> [OPTIONS]

{}",
        options_str()
    )
}

fn log_help_str() -> String {
    format!(
        "kanata_ime_observer log: does not send any request to kanata.

Usage:
    kanata_ime_observer log <PORT> [OPTIONS]

{}",
        options_str()
    )
}

#[derive(Debug)]
pub struct Args {
    pub port: u16,
    pub command: Command,
    pub log_level: Level,
    #[cfg(all(feature = "fcitx", target_os = "linux"))]
    pub app_config: FcitxImeReceiverConfig,

    #[cfg(all(not(feature = "fcitx"), target_os = "linux"))]
    pub app_config: IbusImeReceiverConfig,

    #[cfg(all(feature = "winonoff", target_os = "windows"))]
    pub app_config: WindowsImeOnOffReceiverConfig,

    #[cfg(all(not(feature = "winonoff"), target_os = "windows"))]
    pub app_config: WindowsImeReceiverConfig,

    #[cfg(target_os = "macos")]
    pub app_config: MacImeReceiverConfig,
}

#[allow(unused_mut)]
pub fn parse_args() -> Result<Args, AppError> {
    let mut parser = Parser::from_env();

    #[cfg(all(feature = "fcitx", target_os = "linux"))]
    let mut app_config = FcitxImeReceiverConfig::default();

    #[cfg(all(not(feature = "fcitx"), target_os = "linux"))]
    let mut app_config = IbusImeReceiverConfig::default();

    #[cfg(all(feature = "winonoff", target_os = "windows"))]
    let mut app_config = WindowsImeOnOffReceiverConfig::default();

    #[cfg(all(not(feature = "winonoff"), target_os = "windows"))]
    let mut app_config = WindowsImeReceiverConfig::default();

    #[cfg(target_os = "macos")]
    let mut app_config = MacImeReceiverConfig::default();

    let subcommand = parser.value().map_err(|_|{AppError::ArgError("kanata_ime_observer has three subcommand 'kanata_ime_observer config', 'kanata_ime_observer layer' and 'kanata_ime_observer log'.".to_owned())})?;

    let subcommand_name = match subcommand.to_str() {
        Some("config") => "config",
        Some("layer") => "layer",
        Some("log") => "log",
        Some("-h") | Some("--help") => {
            println!("{}", help_str());
            std::process::exit(0);
        }
        _ => {
            return Err(AppError::ArgError("kanata_ime_observer has three subcommand 'kanata_ime_observer config', 'kanata_ime_observer layer' and 'kanata_ime_observer log'.".to_owned()));
        }
    };

    // 第一引数
    let initial_pos_arg = parser.value().map_err(|_| {
        AppError::ArgError(format!(
            "'kanata_ime_observer {subcommand_name}' needs one positional argument 'PORT'."
        ))
    })?;

    if let Some(initial_pos_arg_str) = initial_pos_arg.to_str()
        && let "-h" | "--help" = initial_pos_arg_str
    {
        match subcommand_name {
            "config" => {
                println!("{}", config_help_str());
            }
            "layer" => {
                println!("{}", layer_help_str());
            }
            "log" => {
                println!("{}", log_help_str());
            }
            _ => {}
        }
        std::process::exit(0);
    }

    // ポート番号の取得
    let port: u16 = initial_pos_arg
        .parse()
        .map_err(|_| AppError::ArgError("Invalid port number.".to_owned()))?;

    // その他のデフォルト値など
    let mut log_level = Level::Info;

    // for config
    let mut config_map: HashMap<String, usize> = HashMap::new();

    // for layer
    let mut ime_names: Vec<String> = Vec::new();
    let mut layer_names: Vec<String> = Vec::new();

    while let Some(arg) = parser.next()? {
        match arg {
            Short('i') | Long("ime") => {
                let ime_name = parser
                    .value()?
                    .to_str()
                    .ok_or(AppError::ArgError(
                        "This IME name has invalid unicode string.".to_string(),
                    ))?
                    .to_string();

                match subcommand_name {
                    "config" => {
                        let config_number = config_map.len();
                        if config_map.insert(ime_name, config_number).is_some() {
                            return Err(AppError::ArgError("Duplicate IME name.".to_string()));
                        }
                    }
                    "layer" => {
                        ime_names.push(ime_name);
                    }
                    _ => return Err(AppError::ArgError("Unexpected option.".to_string())),
                }
            }
            Short('l') | Long("layer") => match subcommand_name {
                "layer" => {
                    let layer_name = parser
                        .value()?
                        .to_str()
                        .ok_or(AppError::ArgError(
                            "This layer name has invalid unicode string.".to_owned(),
                        ))?
                        .to_string();
                    layer_names.push(layer_name);
                }
                _ => return Err(AppError::ArgError("Unexpected option.".to_string())),
            },
            Short('h') | Long("help") => {
                match subcommand_name {
                    "config" => {
                        println!("{}", config_help_str());
                    }
                    "layer" => {
                        println!("{}", layer_help_str());
                    }
                    "log" => {
                        println!("{}", log_help_str());
                    }
                    _ => {}
                }
                std::process::exit(0);
            }
            Short('d') | Long("debug") => {
                log_level = Level::Debug;
            }
            #[cfg(target_os = "windows")]
            Long("polling") => {
                let polling_span: u64 = parser.value()?.parse()?;
                app_config.polling_span = Some(polling_span);
            }
            #[cfg(target_os = "windows")]
            Long("without-polling") => {
                app_config.polling_span = None;
            }
            #[cfg(all(feature = "winonoff", target_os = "windows"))]
            Long("retry-number") => {
                let retry_number: usize = parser.value()?.parse()?;
                app_config.retry_number = retry_number;
            }
            #[cfg(all(feature = "winonoff", target_os = "windows"))]
            Long("sendmessage-timeout") => {
                let send_message_timeout: u32 = parser.value()?.parse()?;
                app_config.send_message_timeout = send_message_timeout;
            }
            #[cfg(all(feature = "winonoff", target_os = "windows"))]
            Long("retry-span") => {
                let retry_span: u64 = parser.value()?.parse()?;
                app_config.retry_span = retry_span;
            }
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            Long("delay") => {
                let delay: u64 = parser.value()?.parse()?;
                app_config.delay = delay;
            }
            _ => Err(arg.unexpected())?,
        }
    }

    match subcommand_name {
        "config" => {
            if config_map.is_empty() {
                println!("{}", config_help_str());
                std::process::exit(0);
            }

            Ok(Args {
                port,
                command: Command::Config(config_map),
                log_level,
                app_config,
            })
        }
        "layer" => {
            if ime_names.len() != layer_names.len() {
                return Err(AppError::ArgError("'kanata_ime_observer layer' needs the same number of IME names and layer names.".to_string()));
            }

            let mut layer_map: HashMap<String, String> = HashMap::new();
            for (ime_name, layer_name) in ime_names.into_iter().zip(layer_names.into_iter()) {
                if layer_map.insert(ime_name, layer_name).is_some() {
                    return Err(AppError::ArgError("Duplicate IME name.".to_string()));
                }
            }

            Ok(Args {
                port,
                command: Command::Layer(layer_map),
                log_level,
                app_config,
            })
        }
        "log" => Ok(Args {
            port,
            command: Command::Log,
            log_level,
            app_config,
        }),
        _ => {
            unreachable!();
        }
    }
}
