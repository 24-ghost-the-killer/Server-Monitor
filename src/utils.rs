#[cfg(windows)]
pub fn setup_console() {
    use windows_sys::Win32::System::Console::{
        GetStdHandle, GetConsoleMode, SetConsoleMode, SetConsoleOutputCP,
        STD_OUTPUT_HANDLE, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    };
    unsafe {
        SetConsoleOutputCP(65001);
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0;
        if GetConsoleMode(handle, &mut mode) != 0 {
            SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

#[cfg(not(windows))]
pub fn setup_console() {}
