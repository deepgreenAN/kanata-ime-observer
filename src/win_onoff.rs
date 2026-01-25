use crate::{
    AppError, FatalError, InnerReceiver, Message, MessageReceiver, handle_try_send,
    send_fatal_error, send_message,
};

use std::sync::mpsc::sync_channel;
use std::time::Duration;

use windows::Win32::{
    Devices::HumanInterfaceDevice::{HID_USAGE_GENERIC_KEYBOARD, HID_USAGE_PAGE_GENERIC},
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        Input::{
            GetRawInputData, HRAWINPUT, Ime::ImmGetDefaultIMEWnd, RAWINPUT, RAWINPUTDEVICE,
            RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIM_TYPEKEYBOARD, RegisterRawInputDevices,
        },
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, EVENT_SYSTEM_FOREGROUND,
            GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetMessageW,
            GetWindowThreadProcessId, HWND_MESSAGE, MSG, PostQuitMessage, RegisterClassExW,
            SMTO_ABORTIFHUNG, SMTO_NORMAL, SW_HIDE, SendMessageTimeoutW, ShowWindow,
            TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WM_DESTROY,
            WM_IME_CONTROL, WM_INPUT, WM_KEYDOWN, WNDCLASSEXW,
        },
    },
};

use windows::core::w;

use log::{debug, error};

const IMC_GETOPENSTATUS: usize = 0x0005;

const VK_IME_ON: u16 = windows::Win32::UI::Input::KeyboardAndMouse::VK_IME_ON.0 as _; // RawInputでは出力されない
const VK_IME_OFF: u16 = windows::Win32::UI::Input::KeyboardAndMouse::VK_IME_OFF.0 as _; // RawInputでは出力されない
const VK_JP_IME_ON: u16 = 244; // 日本語用
const VK_JP_IME_OFF: u16 = 243; // 日本語用
const VK_JP_EISU: u16 = 240; // 日本語用
const VK_HANGUL: u16 = windows::Win32::UI::Input::KeyboardAndMouse::VK_HANGUL.0 as _; // ハングル用

/// フォーカス変更時に実行されるコールバック
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

// SendMessageを行うため、必ずUIスレッド、フックなどとは異なるスレッドから呼ぶ。
fn get_window_ime_status(
    retry_number: usize,
    send_message_timeout: u32,
    retry_span: u64,
) -> Result<String, AppError> {
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

        let target_hwnd = if GetGUIThreadInfo(thread_id, &mut gui_info).is_ok()
            && !gui_info.hwndFocus.is_invalid()
        {
            gui_info.hwndFocus
        } else {
            foreground_hwnd
        };

        // IME管理ウィンドウの取得
        let target_hwnd_ime = ImmGetDefaultIMEWnd(target_hwnd);

        if target_hwnd_ime.is_invalid() {
            return Err(AppError::WinApiError(
                "The result of ImmGetDefaultIMEWnd is invalid".to_string(),
            ));
        }

        let response = {
            let mut response: Option<usize> = None;
            for _ in 0..retry_number {
                let mut result: usize = 0;

                let res = SendMessageTimeoutW(
                    target_hwnd_ime,
                    WM_IME_CONTROL,
                    WPARAM(IMC_GETOPENSTATUS),
                    LPARAM(0),
                    SMTO_NORMAL | SMTO_ABORTIFHUNG,
                    send_message_timeout,
                    Some(&mut result),
                );

                if res.0 != 0 {
                    response = Some(result);
                    break;
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(retry_span));
                }
            }
            response
        };

        response
            .ok_or(AppError::WinApiError(
                "Could not SendMessageTimeoutW.".to_string(),
            ))
            .map(|res| {
                if res != 0 {
                    "ime-on".to_string()
                } else {
                    "ime-off".to_string()
                }
            })
    }
}

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

                        if keyboard.Message == WM_KEYDOWN {
                            // println!("keyboard.Vkey: {}", keyboard.VKey);
                            // println!("kayboard.MakeCode: {}", keyboard.MakeCode);

                            // kanataのバグ？でgrvではVK_IME_ONのみ出力するため、その都度問い合わせる。
                            if let VK_JP_IME_ON | VK_JP_IME_OFF | VK_JP_EISU | VK_IME_ON
                            | VK_IME_OFF | VK_HANGUL = keyboard.VKey
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

        RegisterClassExW(&wc);

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

    if fatal_error.is_none() {
        Err(AppError::WinApiError("WM_QUIT received.".to_string()))
    } else {
        Err(AppError::CaughtFatalError {
            location: "win_main_loop".to_string(),
        })
    }
}

#[derive(Debug)]
pub struct WindowsImeOnOffReceiverConfig {
    pub retry_number: usize,
    pub send_message_timeout: u32,
    pub retry_span: u64,
    pub delay: u64,
    pub polling_span: Option<u64>,
}

impl Default for WindowsImeOnOffReceiverConfig {
    fn default() -> Self {
        Self {
            retry_number: 3,
            send_message_timeout: 100,
            retry_span: 100,
            delay: 50,
            polling_span: Some(1000),
        }
    }
}

pub struct WindowsImeOnOffReceiver {
    _worker_handle: std::thread::JoinHandle<MessageReceiver>,
    _polling_handle: Option<std::thread::JoinHandle<()>>,
    inner_receiver: InnerReceiver,
    pre_ime_status: Option<String>,
}

impl WindowsImeOnOffReceiver {
    pub fn new(
        message_receiver: MessageReceiver,
        config: &WindowsImeOnOffReceiverConfig,
        fatal_error: &FatalError,
    ) -> Result<Self, AppError> {
        let WindowsImeOnOffReceiverConfig {
            retry_number,
            send_message_timeout,
            retry_span,
            delay,
            polling_span,
        } = config;

        let (inner_sender, inner_receiver) = sync_channel(1);

        // workerスレッド
        let _worker_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();
            let delay = *delay;
            let retry_number = *retry_number;
            let send_message_timeout = *send_message_timeout;
            let retry_span = *retry_span;

            move || {
                while let Ok(_msg) = message_receiver.recv()
                    && fatal_error.is_none()
                {
                    std::thread::sleep(std::time::Duration::from_millis(delay)); // IME変更を反映させるため

                    match get_window_ime_status(retry_number, send_message_timeout, retry_span) {
                        Ok(response) => {
                            handle_try_send(
                                &inner_sender,
                                response,
                                "WindowsImeOnOffReceiver inner sender.".to_string(),
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
                        receiver_name: "WindowsImeOnOffReceiver inner receiver".to_string(),
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
        debug!("WinOnOffImeReceiver shutdown.");

        message_receiver
    }
}
