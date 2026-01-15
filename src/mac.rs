use crate::{
    AppError, FatalError, InnerReceiver, Message, MessageReceiver, handle_try_send,
    send_fatal_error, send_message,
};

use std::ffi::c_void;
use std::sync::mpsc::sync_channel;
use std::time::Duration;

use core_foundation::{
    base::{CFRelease, CFType, CFTypeRef, TCFType},
    dictionary::CFDictionaryRef,
    runloop::{CFRunLoop, CFRunLoopRunResult, kCFRunLoopDefaultMode},
    string::{CFString, CFStringRef},
};
use core_foundation_sys::notification_center::{
    CFNotificationCenterAddObserver, CFNotificationCenterGetDistributedCenter,
    CFNotificationCenterRef, CFNotificationCenterRemoveObserver, CFNotificationName,
    CFNotificationSuspensionBehavior,
};
use log::{debug, error};

type TISInputSourceRef = *const c_void;

#[allow(non_upper_case_globals)]
const CFNotificationSuspensionBehaviorDeliverImmediately: CFNotificationSuspensionBehavior = 4;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    static kTISPropertyInputSourceID: CFStringRef;

    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISGetInputSourceProperty(
        input_source: TISInputSourceRef,
        property_key: CFStringRef,
    ) -> CFTypeRef; // CFStringRef以外も取りうる

    static kTISNotifySelectedKeyboardInputSourceChanged: CFStringRef;
}

/// 入力ソースが変更されるごとに呼ばれるコールバック
extern "C" fn callback(
    _center: CFNotificationCenterRef,
    _observer: *mut c_void,
    _name: CFNotificationName,
    _object: *const c_void,
    _user_info: CFDictionaryRef,
) {
    send_message(Message::GetImeStatus);
}

fn get_current_input_source() -> Result<String, AppError> {
    unsafe {
        let source = TISCopyCurrentKeyboardInputSource();

        let input_source = CFType::wrap_under_get_rule(TISGetInputSourceProperty(
            source,
            kTISPropertyInputSourceID,
        ))
        .downcast_into::<CFString>()
        .ok_or(AppError::MacApiError(
            "Internal bug. Type miss match.".to_string(),
        ))?
        .to_string();

        CFRelease(source);

        Ok(input_source)
    }
}

/// macのメインループ
pub fn mac_main_loop(fatal_error: &FatalError) -> Result<(), AppError> {
    unsafe {
        let observer_ptr = Box::into_raw(Box::new(1)); // observer自体はなんでも良い

        let notify_center = CFNotificationCenterGetDistributedCenter();

        CFNotificationCenterAddObserver(
            notify_center,
            observer_ptr as _,
            callback,
            kTISNotifySelectedKeyboardInputSourceChanged,
            std::ptr::null(),
            CFNotificationSuspensionBehaviorDeliverImmediately,
        );

        // run_loop
        while let run_result =
            CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_secs(1), true)
            && run_result != CFRunLoopRunResult::Stopped
            && fatal_error.is_none()
        {}

        // 終了処理
        CFNotificationCenterRemoveObserver(
            notify_center,
            observer_ptr as _,
            kTISNotifySelectedKeyboardInputSourceChanged,
            std::ptr::null(),
        );
        let _ = Box::from_raw(observer_ptr);
    }

    Err(AppError::CustomError(
        "mac_main_loop: Caught the fatal error.".to_string(),
    ))
}

#[derive(Debug)]
pub struct MacImeReceiverConfig {
    pub delay: u64,
}

impl Default for MacImeReceiverConfig {
    fn default() -> Self {
        Self { delay: 50 }
    }
}

pub struct MacImeReceiver {
    _worker_handle: std::thread::JoinHandle<MessageReceiver>,
    inner_receiver: InnerReceiver,
    pre_ime_status: Option<String>,
}

impl MacImeReceiver {
    pub fn new(
        message_receiver: MessageReceiver,
        config: &MacImeReceiverConfig,
        fatal_error: &FatalError,
    ) -> Result<Self, AppError> {
        let MacImeReceiverConfig { delay } = config;

        let (inner_sender, inner_receiver) = sync_channel(1);

        let _worker_handle = std::thread::spawn({
            let fatal_error = fatal_error.clone();
            let delay = *delay;

            move || {
                while let Ok(_msg) = message_receiver.recv()
                    && fatal_error.is_none()
                {
                    std::thread::sleep(Duration::from_millis(delay)); // 短時間に複数回呼ぶことを防ぐ。

                    match get_current_input_source() {
                        Ok(ime_status) => handle_try_send(
                            &inner_sender,
                            ime_status,
                            "MacImeReceiver inner sender".to_string(),
                        ),
                        Err(e) => {
                            error!("{e}");
                        }
                    }
                }

                message_receiver
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
                        receiver_name: "MacImeReceiver inner receiver".to_string(),
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
        debug!("MacImeReceiver shutdown.");

        message_receiver
    }
}
