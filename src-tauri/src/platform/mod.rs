pub mod autostart;
pub mod elevation;

use windows::Win32::Globalization::GetUserDefaultUILanguage;

/// "ru" when the Windows UI language is Russian, otherwise "en".
pub fn system_language() -> &'static str {
    const LANG_RUSSIAN: u16 = 0x19;
    let lang = unsafe { GetUserDefaultUILanguage() };
    if lang & 0x3ff == LANG_RUSSIAN { "ru" } else { "en" }
}
