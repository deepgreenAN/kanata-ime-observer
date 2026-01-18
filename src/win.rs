use crate::{
    AppError, FatalError, InnerReceiver, Message, MessageReceiver, handle_try_send,
    send_fatal_error, send_message,
};

use std::{collections::HashMap, sync::mpsc::sync_channel, time::Duration};

use windows::Win32::{
    Devices::HumanInterfaceDevice::{HID_USAGE_GENERIC_KEYBOARD, HID_USAGE_PAGE_GENERIC},
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Globalization::LCIDToLocaleName,
    System::{LibraryLoader::GetModuleHandleW, SystemServices::LOCALE_NAME_MAX_LENGTH},
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        Input::{
            GetRawInputData, HRAWINPUT,
            KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL},
            RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIM_TYPEKEYBOARD,
            RegisterRawInputDevices,
        },
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, EVENT_SYSTEM_FOREGROUND,
            GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetMessageW,
            GetWindowThreadProcessId, HWND_MESSAGE, MSG, PostQuitMessage, RegisterClassExW,
            SW_HIDE, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE,
            WINEVENT_OUTOFCONTEXT, WM_DESTROY, WM_INPUT, WM_KEYUP, WNDCLASSEXW,
        },
    },
};
use windows::core::w;

use log::{debug, error};

const VK_CONTROL: u16 = windows::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL.0 as _;
const VK_LCONTROL: u16 = windows::Win32::UI::Input::KeyboardAndMouse::VK_LCONTROL.0 as _; // RawInputではおそらく出力されない
const VK_RCONTROL: u16 = windows::Win32::UI::Input::KeyboardAndMouse::VK_RCONTROL.0 as _; // RawInputではおそらく出力されない
const VK_LWIN: u16 = windows::Win32::UI::Input::KeyboardAndMouse::VK_LWIN.0 as _;
const VK_RWIN: u16 = windows::Win32::UI::Input::KeyboardAndMouse::VK_RWIN.0 as _;

/// フォーカス変更時に実行されるコールバック。これはキーによる変更でもほとんどの場合で呼ばれるが、入力メソッド変更の小ウィンドウに対して行われるため、長押しした場合などに想定した挙動とはならない。
extern "system" fn win_event_proc(
    _hwineventhook: HWINEVENTHOOK,
    event: u32,
    _hwnd: HWND,
    _idobject: i32,
    _idchild: i32,
    _ideventthread: u32,
    _dwmseventtime: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND {
        send_message(Message::GetImeStatus);
    }
}

/// 特定の修飾キーの押下を検出する。実際には入力メソッドの変更をキーで行うとフォーカスの変更も行われることに注意する。
extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_INPUT => {
                let mut raw_input_size = 0_u32;
                GetRawInputData(
                    HRAWINPUT(lparam.0 as _),
                    RID_INPUT,
                    None,
                    &mut raw_input_size,
                    std::mem::size_of::<RAWINPUTHEADER>() as u32,
                ); // サイズを取得する

                let mut buf = vec![0_u8; raw_input_size as usize];
                if GetRawInputData(
                    HRAWINPUT(lparam.0 as _),
                    RID_INPUT,
                    Some(buf.as_mut_ptr() as _),
                    &mut raw_input_size,
                    std::mem::size_of::<RAWINPUTHEADER>() as u32,
                ) == raw_input_size
                {
                    let raw_input = &*(buf.as_ptr() as *const RAWINPUT);
                    if raw_input.header.dwType == RIM_TYPEKEYBOARD.0 {
                        let keyboard = raw_input.data.keyboard;

                        if keyboard.Message == WM_KEYUP {
                            // println!("keyboard.Vkey: {}", keyboard.VKey);
                            // println!("kayboard.MakeCode: {}", keyboard.MakeCode);
                            // 修飾キーであった場合
                            if let VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_LWIN | VK_RWIN =
                                keyboard.VKey
                            {
                                send_message(Message::GetImeStatus);
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

// winのメインループ。
pub fn win_main_loop(fatal_error: &FatalError) -> Result<(), AppError> {
    unsafe {
        // hook
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );

        // window
        let hinstance = GetModuleHandleW(None)?;

        let class_name = w!("ImeObserverWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        let atom = RegisterClassExW(&wc);
        debug_assert!(atom != 0);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("instance"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance.into()),
            None,
        )?;

        let rid = RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: HID_USAGE_GENERIC_KEYBOARD,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };

        RegisterRawInputDevices(&[rid], std::mem::size_of::<RAWINPUTDEVICE>() as u32)?;

        let _ = ShowWindow(hwnd, SW_HIDE);

        // メッセージループ
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() && fatal_error.is_none() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if !hook.is_invalid() {
            let _ = UnhookWinEvent(hook);
        }
    }

    Ok(())
}

// initialize_locale_map内でのみ利用する。
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
fn initialize_locale_map() -> Result<HashMap<u16, String>, AppError> {
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

        if hkl.0 as usize == 0 {
            return Err(AppError::WinApiError(
                "Invalid HKL. It may happen when you use a console application.".to_string(),
            ));
        }

        match locale_map.get(&((hkl.0 as usize & 0xFFFF) as u16)) {
            Some(locale) => Ok(locale.to_owned()),
            None => Err(AppError::WinApiError("Unknown lang_id".to_string())),
        }
    }
}

#[derive(Debug)]
pub struct WindowsImeReceiverConfig {
    pub delay: u64,
    pub polling_span: Option<u64>,
}

impl Default for WindowsImeReceiverConfig {
    fn default() -> Self {
        Self {
            delay: 50,
            polling_span: Some(500),
        }
    }
}

pub struct WindowsImeReceiver {
    _worker_handle: std::thread::JoinHandle<MessageReceiver>,
    _polling_handle: Option<std::thread::JoinHandle<()>>,
    inner_receiver: InnerReceiver,
    pre_ime_status: Option<String>,
}

impl WindowsImeReceiver {
    pub fn new(
        message_receiver: MessageReceiver,
        config: &WindowsImeReceiverConfig,
        fatal_error: &FatalError,
    ) -> Result<Self, AppError> {
        let WindowsImeReceiverConfig {
            delay,
            polling_span,
        } = config;

        let (inner_sender, inner_receiver) = sync_channel(1);

        let locale_map = initialize_locale_map()?;

        let _worker_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();
            let delay = *delay;

            move || {
                while let Ok(_msg) = message_receiver.recv()
                    && fatal_error.is_none()
                {
                    std::thread::sleep(std::time::Duration::from_millis(delay)); // IME変更を反映させるため

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

                message_receiver
            }
        });

        // ポーリングスレッド
        let _polling_handle = polling_span.map(|polling_span| {
            std::thread::spawn({
                let fatal_error = fatal_error.clone();

                move || {
                    while fatal_error.is_none() {
                        std::thread::sleep(Duration::from_millis(polling_span));
                        send_message(Message::GetImeStatus);
                    }
                }
            })
        });

        Ok(Self {
            _worker_handle,
            _polling_handle,
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

    pub fn shutdown(self) -> MessageReceiver {
        send_fatal_error(AppError::CustomError("Receiver shutdown.".to_string()));
        let message_receiver = self._worker_handle.join().expect("worker thread panicked.");
        if let Some(_polling_handle) = self._polling_handle {
            _polling_handle.join().expect("polling thread panicked.");
        }
        debug!("WinImeReceiver shutdown.");

        message_receiver
    }
}
