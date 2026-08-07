//! Native Win32 Desktop GUI Client for Windows
//!
//! Provides a classic Windows GUI softphone interface for users who prefer
//! a traditional desktop application over CLI commands or Web Dashboard.

#[cfg(not(windows))]
pub async fn run_gui(_config_path: String, _ctrl_port: u16) -> anyhow::Result<()> {
    anyhow::bail!("The Win32 GUI client is only available on Windows operating systems.");
}

#[cfg(windows)]
pub use windows_impl::run_gui;

#[cfg(windows)]
mod windows_impl {
    use crate::config::Config;
    use crate::ipc::{Request, Response};
    use crate::ipc_client;
    use crate::service;
    use anyhow::Result;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Sender};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Controls::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    // Control IDs
    const ID_CMB_ACCOUNT: usize = 1001;
    const ID_BTN_REGISTER: usize = 1002;
    const ID_TXT_TARGET: usize = 1003;
    const ID_BTN_CALL: usize = 1004;
    const ID_BTN_HANGUP: usize = 1005;
    const ID_BTN_HOLD: usize = 1006;
    const ID_BTN_RESUME: usize = 1007;
    const ID_BTN_TRANSFER: usize = 1008;
    const ID_TXT_WAV: usize = 1009;
    const ID_BTN_PLAY: usize = 1010;
    const ID_BTN_WEB: usize = 1011;
    const ID_TXT_LOG: usize = 1012;
    const _ID_LBL_STATUS: usize = 1013;

    // Dialpad button IDs
    const ID_BTN_D_1: usize = 1021;
    const ID_BTN_D_2: usize = 1022;
    const ID_BTN_D_3: usize = 1023;
    const ID_BTN_D_4: usize = 1024;
    const ID_BTN_D_5: usize = 1025;
    const ID_BTN_D_6: usize = 1026;
    const ID_BTN_D_7: usize = 1027;
    const ID_BTN_D_8: usize = 1028;
    const ID_BTN_D_9: usize = 1029;
    const ID_BTN_D_STAR: usize = 1030;
    const ID_BTN_D_0: usize = 1031;
    const ID_BTN_D_HASH: usize = 1032;

    static SERVICE_RUNNING: AtomicBool = AtomicBool::new(false);
    static mut HWND_MAIN: HWND = 0;
    static mut HWND_LOG: HWND = 0;
    static mut HWND_STATUS: HWND = 0;
    static mut HWND_TARGET: HWND = 0;
    static mut HWND_ACCOUNT: HWND = 0;
    static mut HWND_WAV: HWND = 0;

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn append_log(msg: &str) {
        unsafe {
            if HWND_LOG == 0 {
                return;
            }
            let time_str = chrono_time();
            let formatted = format!("[{}] {}\r\n", time_str, msg);
            let wide = to_wide(&formatted);

            // Get current text length
            let len = GetWindowTextLengthW(HWND_LOG);
            // Set selection to end
            SendMessageW(HWND_LOG, EM_SETSEL, len as usize, len as isize);
            // Replace selection with new log line
            SendMessageW(HWND_LOG, EM_REPLACESEL, 0, wide.as_ptr() as LPARAM);
            // Auto-scroll
            SendMessageW(HWND_LOG, WM_VSCROLL, SB_BOTTOM as usize, 0);
        }
    }

    fn chrono_time() -> String {
        use std::time::SystemTime;
        if let Ok(duration) = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
            let secs = duration.as_secs();
            let hours = (secs / 3600 % 24) + 3; // UTC+3 / local offset
            let mins = (secs / 60) % 60;
            let s = secs % 60;
            format!("{:02}:{:02}:{:02}", hours, mins, s)
        } else {
            "00:00:00".to_string()
        }
    }

    fn set_status_text(text: &str) {
        unsafe {
            if HWND_STATUS != 0 {
                let wide = to_wide(text);
                SetWindowTextW(HWND_STATUS, wide.as_ptr());
            }
        }
    }

    fn get_control_text(hwnd: HWND) -> String {
        unsafe {
            if hwnd == 0 {
                return String::new();
            }
            let len = GetWindowTextLengthW(hwnd);
            if len == 0 {
                return String::new();
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);
            String::from_utf16_lossy(&buf[..len as usize])
        }
    }

    fn get_selected_account() -> String {
        unsafe {
            if HWND_ACCOUNT == 0 {
                return String::new();
            }
            let idx = SendMessageW(HWND_ACCOUNT, CB_GETCURSEL, 0, 0);
            if idx < 0 {
                return String::new();
            }
            let len = SendMessageW(HWND_ACCOUNT, CB_GETLBTEXTLEN, idx as usize, 0);
            if len <= 0 {
                return String::new();
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            SendMessageW(HWND_ACCOUNT, CB_GETLBTEXT, idx as usize, buf.as_mut_ptr() as LPARAM);
            String::from_utf16_lossy(&buf[..len as usize])
        }
    }

    pub async fn run_gui(config_path: String, ctrl_port: u16) -> Result<()> {
        let cfg = match Config::load(&config_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: Failed to load config ({}), fallback to default empty config...", e);
                Config::load("config.toml").unwrap_or_else(|_| Config {
                    web: None,
                    commands_api: None,
                    syslog: None,
                    accounts: vec![],
                    plugins: None,
                })
            }
        };

        // Check if service is running by attempting status IPC
        let req = Request::new("status");
        let service_already_running = match ipc_client::send_ipc(&req, ctrl_port).await {
            Ok(resp) => resp.ok,
            Err(_) => false,
        };

        if !service_already_running {
            println!("Service engine not detected on port {}. Launching embedded SIP service engine...", ctrl_port);
            let cfg_clone = cfg.clone();
            let config_path_clone = config_path.clone();
            tokio::spawn(async move {
                match service::Service::new(&cfg_clone, ctrl_port, config_path_clone).await {
                    Ok(svc) => {
                        SERVICE_RUNNING.store(true, Ordering::SeqCst);
                        if let Err(e) = svc.run().await {
                            eprintln!("Embedded service error: {}", e);
                        }
                    }
                    Err(e) => eprintln!("Failed to initialize embedded SIP service: {}", e),
                }
            });
            // Give embedded Tokio engine a moment to bind ports
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        } else {
            SERVICE_RUNNING.store(true, Ordering::SeqCst);
            println!("Connected to existing background SIP service on port {}.", ctrl_port);
        }

        // We run the Win32 GUI message loop on a dedicated thread
        let (tx, rx) = channel::<GuiAction>();
        let accounts: Vec<String> = cfg.accounts.iter().map(|a| a.name.clone()).collect();
        let port = ctrl_port;
        let web_port = cfg.web_port();

        std::thread::spawn(move || {
            unsafe {
                let h_instance = GetModuleHandleW(std::ptr::null());
                let class_name = to_wide("RSIPClientWin32GUIClass");

                let wnd_class = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: h_instance,
                    hIcon: LoadIconW(0, IDI_APPLICATION),
                    hCursor: LoadCursorW(0, IDC_ARROW),
                    hbrBackground: (COLOR_BTNFACE + 1) as HBRUSH,
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: class_name.as_ptr(),
                };

                RegisterClassW(&wnd_class);

                let title = to_wide("rsipclient — Classic Windows SIP Softphone (Win32)");
                let hwnd = CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    title.as_ptr(),
                    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    560,
                    640,
                    0,
                    0,
                    h_instance,
                    std::ptr::null(),
                );

                HWND_MAIN = hwnd;

                // Set GUI Font
                let h_font = GetStockObject(DEFAULT_GUI_FONT) as HFONT;

                // 1. Account Combobox & Register Button
                let _lbl_acc = create_label(hwnd, "SIP Account:", 20, 20, 90, 22, h_font, h_instance);
                let hwnd_acc = CreateWindowExW(
                    0,
                    to_wide("COMBOBOX").as_ptr(),
                    std::ptr::null(),
                    WS_CHILD | WS_VISIBLE | (CBS_DROPDOWNLIST as u32) | WS_VSCROLL,
                    115,
                    18,
                    250,
                    150,
                    hwnd,
                    ID_CMB_ACCOUNT as HMENU,
                    h_instance,
                    std::ptr::null(),
                );
                HWND_ACCOUNT = hwnd_acc;
                SendMessageW(hwnd_acc, WM_SETFONT, h_font as usize, 1);

                for acc in &accounts {
                    let w_acc = to_wide(acc);
                    SendMessageW(hwnd_acc, CB_ADDSTRING, 0, w_acc.as_ptr() as LPARAM);
                }
                if !accounts.is_empty() {
                    SendMessageW(hwnd_acc, CB_SETCURSEL, 0, 0);
                }

                create_button(hwnd, "Register", 375, 17, 145, 26, ID_BTN_REGISTER, h_font, h_instance);

                // 2. Status Label
                let hwnd_st = create_label(hwnd, "Status: Ready (Idle)", 20, 52, 500, 22, h_font, h_instance);
                HWND_STATUS = hwnd_st;

                // 3. Target URI / Number Input Box
                create_label(hwnd, "Target / Dial Number:", 20, 85, 140, 22, h_font, h_instance);
                let hwnd_tgt = CreateWindowExW(
                    WS_EX_CLIENTEDGE,
                    to_wide("EDIT").as_ptr(),
                    to_wide("1002").as_ptr(),
                    WS_CHILD | WS_VISIBLE | (ES_AUTOHSCROLL as u32),
                    165,
                    82,
                    355,
                    25,
                    hwnd,
                    ID_TXT_TARGET as HMENU,
                    h_instance,
                    std::ptr::null(),
                );
                HWND_TARGET = hwnd_tgt;
                SendMessageW(hwnd_tgt, WM_SETFONT, h_font as usize, 1);

                // 4. Action Buttons (Call, Hangup, Hold, Resume, Transfer)
                create_button(hwnd, "📞 CALL", 20, 120, 95, 32, ID_BTN_CALL, h_font, h_instance);
                create_button(hwnd, "🛑 HANGUP", 122, 120, 95, 32, ID_BTN_HANGUP, h_font, h_instance);
                create_button(hwnd, "⏸️ HOLD", 224, 120, 95, 32, ID_BTN_HOLD, h_font, h_instance);
                create_button(hwnd, "▶️ RESUME", 326, 120, 95, 32, ID_BTN_RESUME, h_font, h_instance);
                create_button(hwnd, "↗️ TRANSFER", 428, 120, 92, 32, ID_BTN_TRANSFER, h_font, h_instance);

                // 5. Classic 3x4 Dialpad Grid
                let dp_x = 180;
                let dp_y = 165;
                let btn_w = 60;
                let btn_h = 32;
                let pad = 8;

                let dial_buttons = [
                    ("1", ID_BTN_D_1, 0, 0), ("2", ID_BTN_D_2, 1, 0), ("3", ID_BTN_D_3, 2, 0),
                    ("4", ID_BTN_D_4, 0, 1), ("5", ID_BTN_D_5, 1, 1), ("6", ID_BTN_D_6, 2, 1),
                    ("7", ID_BTN_D_7, 0, 2), ("8", ID_BTN_D_8, 1, 2), ("9", ID_BTN_D_9, 2, 2),
                    ("*", ID_BTN_D_STAR, 0, 3), ("0", ID_BTN_D_0, 1, 3), ("#", ID_BTN_D_HASH, 2, 3),
                ];

                for (label, id, col, row) in dial_buttons {
                    let bx = dp_x + col * (btn_w + pad);
                    let by = dp_y + row * (btn_h + pad);
                    create_button(hwnd, label, bx, by, btn_w, btn_h, id, h_font, h_instance);
                }

                // 6. Audio Playback Row
                create_label(hwnd, "WAV File:", 20, 335, 75, 22, h_font, h_instance);
                let hwnd_wav = CreateWindowExW(
                    WS_EX_CLIENTEDGE,
                    to_wide("EDIT").as_ptr(),
                    to_wide("audio/welcome.wav").as_ptr(),
                    WS_CHILD | WS_VISIBLE | (ES_AUTOHSCROLL as u32),
                    95,
                    332,
                    280,
                    25,
                    hwnd,
                    ID_TXT_WAV as HMENU,
                    h_instance,
                    std::ptr::null(),
                );
                HWND_WAV = hwnd_wav;
                SendMessageW(hwnd_wav, WM_SETFONT, h_font as usize, 1);

                create_button(hwnd, "🎵 Play Audio", 385, 331, 135, 27, ID_BTN_PLAY, h_font, h_instance);

                // 7. Web Dashboard Button
                create_button(hwnd, "🌐 Open Web Dashboard", 20, 370, 500, 28, ID_BTN_WEB, h_font, h_instance);

                // 8. Log / Activity Text Box
                create_label(hwnd, "Real-time Activity & SIP Call Logs:", 20, 405, 300, 18, h_font, h_instance);
                let hwnd_log = CreateWindowExW(
                    WS_EX_CLIENTEDGE,
                    to_wide("EDIT").as_ptr(),
                    std::ptr::null(),
                    WS_CHILD | WS_VISIBLE | (ES_MULTILINE as u32) | (ES_READONLY as u32) | WS_VSCROLL | (ES_AUTOVSCROLL as u32),
                    20,
                    425,
                    500,
                    150,
                    hwnd,
                    ID_TXT_LOG as HMENU,
                    h_instance,
                    std::ptr::null(),
                );
                HWND_LOG = hwnd_log;
                SendMessageW(hwnd_log, WM_SETFONT, h_font as usize, 1);

                let web_url = format!("http://127.0.0.1:{}", web_port);
                append_log("rsipclient Classic Win32 GUI initialized.");
                append_log(&format!("Engine mode: Embedded SIP Service + Web Dashboard ({}) + JSON IPC.", web_url));

                GLOBAL_TX = Some(tx);

                // Message Loop
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });

        // Main thread handles IPC commands sent from Win32 GUI actions
        while let Ok(action) = rx.recv() {
            match action {
                GuiAction::Register => {
                    let account = get_selected_account();
                    if account.is_empty() {
                        append_log("Error: No SIP account selected.");
                        continue;
                    }
                    append_log(&format!("Sending REGISTER request for account '{}'...", account));
                    set_status_text(&format!("Status: Registering '{}'...", account));
                    let req = Request::with_account("register", &account);
                    let resp = ipc_client::send_ipc(&req, port).await;
                    handle_ipc_response(resp, "Register");
                }
                GuiAction::Call => {
                    let account = get_selected_account();
                    let target = get_control_text(unsafe { HWND_TARGET });
                    if account.is_empty() || target.trim().is_empty() {
                        append_log("Error: Please select an account and enter a target SIP URI/number.");
                        continue;
                    }
                    append_log(&format!("Placing outbound call from '{}' to '{}'...", account, target));
                    set_status_text(&format!("Status: Calling {}...", target));
                    let req = Request::with_target("call", &account, &target);
                    let resp = ipc_client::send_ipc(&req, port).await;
                    handle_ipc_response(resp, "Call");
                }
                GuiAction::Hangup => {
                    let account = get_selected_account();
                    append_log(&format!("Sending HANGUP request for account '{}'...", account));
                    set_status_text("Status: Hanging up...");
                    let req = Request::with_account("hangup", &account);
                    let resp = ipc_client::send_ipc(&req, port).await;
                    handle_ipc_response(resp, "Hangup");
                }
                GuiAction::Hold => {
                    let account = get_selected_account();
                    append_log(&format!("Holding call for account '{}'...", account));
                    let req = Request::with_account("hold", &account);
                    let resp = ipc_client::send_ipc(&req, port).await;
                    handle_ipc_response(resp, "Hold");
                }
                GuiAction::Resume => {
                    let account = get_selected_account();
                    append_log(&format!("Resuming call for account '{}'...", account));
                    let req = Request::with_account("resume", &account);
                    let resp = ipc_client::send_ipc(&req, port).await;
                    handle_ipc_response(resp, "Resume");
                }
                GuiAction::Transfer => {
                    let account = get_selected_account();
                    let target = get_control_text(unsafe { HWND_TARGET });
                    if account.is_empty() || target.trim().is_empty() {
                        append_log("Error: Target required for transfer.");
                        continue;
                    }
                    append_log(&format!("Transferring call to '{}'...", target));
                    let req = Request::with_target("transfer", &account, &target);
                    let resp = ipc_client::send_ipc(&req, port).await;
                    handle_ipc_response(resp, "Transfer");
                }
                GuiAction::Dtmf(digit) => {
                    let account = get_selected_account();
                    // Append digit to target text input
                    unsafe {
                        if HWND_TARGET != 0 {
                            let curr = get_control_text(HWND_TARGET);
                            let updated = format!("{}{}", curr, digit);
                            SetWindowTextW(HWND_TARGET, to_wide(&updated).as_ptr());
                        }
                    }
                    append_log(&format!("DTMF Digit: {}", digit));
                    if !account.is_empty() {
                        let req = Request::with_target("dtmf", &account, &digit);
                        let _ = ipc_client::send_ipc(&req, port).await;
                    }
                }
                GuiAction::PlayWav => {
                    let account = get_selected_account();
                    let file = get_control_text(unsafe { HWND_WAV });
                    if file.trim().is_empty() {
                        append_log("Error: WAV file path is empty.");
                        continue;
                    }
                    append_log(&format!("Playing WAV file '{}' over call...", file));
                    let req = Request::with_target("play", &account, &file);
                    let resp = ipc_client::send_ipc(&req, port).await;
                    handle_ipc_response(resp, "Play WAV");
                }
                GuiAction::OpenWeb => {
                    let web_url = format!("http://127.0.0.1:{}", web_port);
                    append_log(&format!("Opening Web Dashboard ({}) in browser...", web_url));
                    let _ = std::process::Command::new("cmd")
                        .args(["/C", "start", &web_url])
                        .spawn();
                }
            }
        }

        Ok(())
    }

    fn handle_ipc_response(res: Result<Response>, action_name: &str) {
        match res {
            Ok(resp) => {
                if resp.ok {
                    append_log(&format!("{}: OK — {}", action_name, resp.msg));
                    set_status_text(&format!("Status: OK — {}", resp.msg));
                } else {
                    append_log(&format!("{}: FAIL — {}", action_name, resp.msg));
                    set_status_text(&format!("Status: Error — {}", resp.msg));
                }
            }
            Err(e) => {
                append_log(&format!("{}: IPC Communication Error — {}", action_name, e));
                set_status_text("Status: Connection Error");
            }
        }
    }

    enum GuiAction {
        Register,
        Call,
        Hangup,
        Hold,
        Resume,
        Transfer,
        Dtmf(String),
        PlayWav,
        OpenWeb,
    }

    static mut GLOBAL_TX: Option<Sender<GuiAction>> = None;

    fn send_action(action: GuiAction) {
        unsafe {
            if let Some(ref tx) = GLOBAL_TX {
                let _ = tx.send(action);
            }
        }
    }

    fn create_button(
        parent: HWND,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        id: usize,
        font: HFONT,
        instance: HINSTANCE,
    ) -> HWND {
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                to_wide("BUTTON").as_ptr(),
                to_wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | (BS_PUSHBUTTON as u32),
                x,
                y,
                w,
                h,
                parent,
                id as HMENU,
                instance,
                std::ptr::null(),
            );
            SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
            hwnd
        }
    }

    fn create_label(
        parent: HWND,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        font: HFONT,
        instance: HINSTANCE,
    ) -> HWND {
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                to_wide("STATIC").as_ptr(),
                to_wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE,
                x,
                y,
                w,
                h,
                parent,
                0,
                instance,
                std::ptr::null(),
            );
            SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
            hwnd
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_COMMAND => {
                let id = (wparam & 0xffff) as usize;
                match id {
                    ID_BTN_REGISTER => send_action(GuiAction::Register),
                    ID_BTN_CALL => send_action(GuiAction::Call),
                    ID_BTN_HANGUP => send_action(GuiAction::Hangup),
                    ID_BTN_HOLD => send_action(GuiAction::Hold),
                    ID_BTN_RESUME => send_action(GuiAction::Resume),
                    ID_BTN_TRANSFER => send_action(GuiAction::Transfer),
                    ID_BTN_D_1 => send_action(GuiAction::Dtmf("1".to_string())),
                    ID_BTN_D_2 => send_action(GuiAction::Dtmf("2".to_string())),
                    ID_BTN_D_3 => send_action(GuiAction::Dtmf("3".to_string())),
                    ID_BTN_D_4 => send_action(GuiAction::Dtmf("4".to_string())),
                    ID_BTN_D_5 => send_action(GuiAction::Dtmf("5".to_string())),
                    ID_BTN_D_6 => send_action(GuiAction::Dtmf("6".to_string())),
                    ID_BTN_D_7 => send_action(GuiAction::Dtmf("7".to_string())),
                    ID_BTN_D_8 => send_action(GuiAction::Dtmf("8".to_string())),
                    ID_BTN_D_9 => send_action(GuiAction::Dtmf("9".to_string())),
                    ID_BTN_D_STAR => send_action(GuiAction::Dtmf("*".to_string())),
                    ID_BTN_D_0 => send_action(GuiAction::Dtmf("0".to_string())),
                    ID_BTN_D_HASH => send_action(GuiAction::Dtmf("#".to_string())),
                    ID_BTN_PLAY => send_action(GuiAction::PlayWav),
                    ID_BTN_WEB => send_action(GuiAction::OpenWeb),
                    _ => {}
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
