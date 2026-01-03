use crate::{AppError, FatalError, handle_try_send, send_fatal_error};

use std::{
    collections::HashMap,
    sync::mpsc::{Receiver, sync_channel},
    time::Duration,
};

use windows::Win32::{
    Globalization::LCIDToLocaleName,
    System::SystemServices::LOCALE_NAME_MAX_LENGTH,
    UI::{
        Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL},
        WindowsAndMessaging::{
            GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
        },
    },
};

use log::error;

// 起動時のみ利用する。
fn lang_id2locale(lang_id: u16) -> Option<String> {
    // MAKELCID(lang_id, SORT_DEFAULT) 相当
    let locale_id = lang_id as u32;

    let mut utf_16_buf = [0_u16; LOCALE_NAME_MAX_LENGTH as usize]; // 暫定的に最大まで作成する

    let written_len = unsafe { LCIDToLocaleName(locale_id, Some(&mut utf_16_buf), 0) };

    if written_len != 0 {
        Some(String::from_utf16_lossy(
            &utf_16_buf[..written_len as usize - 1],
        )) // written_lenから終端文字列を引いたもの
    } else {
        None
    }
}

// Receiverの初期化時に呼ぶ。
fn initialize() -> Result<HashMap<u16, String>, AppError> {
    unsafe {
        // 言語IDのリストを取得する
        let size = GetKeyboardLayoutList(None);

        let mut hkl_list = vec![HKL(std::ptr::null_mut()); size as usize];

        GetKeyboardLayoutList(Some(&mut hkl_list));

        let lang_id_list: Vec<u16> = hkl_list
            .into_iter()
            .map(|hkl| (hkl.0 as usize & 0xFFFF) as u16)
            .collect();

        let mut locale_map: HashMap<u16, String> = HashMap::new();

        for lang_id in lang_id_list.iter() {
            if let Some(locale) = lang_id2locale(*lang_id) {
                locale_map.insert(*lang_id, locale);
            } else {
                return Err(AppError::CustomError("Unknown lang_id".to_string()));
            }
        }

        Ok(locale_map)
    }
}

/// ループの中で呼ぶ。
fn get_foreground_locale(locale_map: &HashMap<u16, String>) -> Result<String, AppError> {
    unsafe {
        let foreground_hwnd = GetForegroundWindow();

        if foreground_hwnd.is_invalid() {
            return Err(AppError::WinApiError(
                "The result of GetForegroundWindow is invalid.".to_string(),
            ));
        }

        let thread_id = GetWindowThreadProcessId(foreground_hwnd, None);

        // 前面ウィンドウのGUIスレッド情報
        let mut gui_info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };

        let target_thread_id = if GetGUIThreadInfo(thread_id, &mut gui_info).is_ok()
            && !gui_info.hwndFocus.is_invalid()
        {
            GetWindowThreadProcessId(gui_info.hwndFocus, None)
        } else {
            thread_id
        };

        let hkl = GetKeyboardLayout(target_thread_id);

        match locale_map.get(&((hkl.0 as usize & 0xFFFF) as u16)) {
            Some(locale) => Ok(locale.to_owned()),
            None => Err(AppError::WinApiError("Unknown lang_id".to_string())),
        }
    }
}

#[derive(Debug)]
pub struct WindowsImeReceiverConfig {
    pub polling_span: u64,
}

impl Default for WindowsImeReceiverConfig {
    fn default() -> Self {
        Self { polling_span: 200 }
    }
}

pub struct WindowsImeReceiver {
    _worker_handle: std::thread::JoinHandle<()>,
    inner_receiver: Receiver<String>,
    pre_ime_status: Option<String>,
}

impl WindowsImeReceiver {
    pub fn new(
        config: &WindowsImeReceiverConfig,
        fatal_error: &FatalError,
    ) -> Result<Self, AppError> {
        let WindowsImeReceiverConfig { polling_span } = config;

        let (inner_sender, inner_receiver) = sync_channel(1);

        let locale_map = initialize()?;

        let _worker_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();
            let polling_span = *polling_span;
            move || {
                while fatal_error.is_none() {
                    std::thread::sleep(Duration::from_millis(polling_span));

                    match get_foreground_locale(&locale_map) {
                        Ok(locale) => {
                            handle_try_send(
                                &inner_sender,
                                locale,
                                "WindowsImeReceiver inner sender.".to_string(),
                            );
                        }
                        Err(e) => {
                            error!("{e}");
                        }
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
                        receiver_name: "WindowsReceiver inner receiver".to_string(),
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
    }
}
