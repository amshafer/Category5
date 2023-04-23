// Constants for input operations
//
// These are taken from SDL2
//
// Austin Shafer - 2020

#[cfg(feature = "direct2display")]
extern crate xkbcommon;
#[cfg(feature = "direct2display")]
use xkbcommon::xkb;

bitflags::bitflags! {
    pub struct Mods: u16 {
        const NONE = 0x0000;
        const LSHIFT = 0x0001;
        const RSHIFT = 0x0002;
        const LCTRL = 0x0040;
        const RCTRL = 0x0080;
        const LALT = 0x0100;
        const RALT = 0x0200;
        const LGUI = 0x0400;
        const RGUI = 0x0800;
        const NUM = 0x1000;
        const CAPS = 0x2000;
        const MODE = 0x4000;
        const RESERVED = 0x8000;
    }
}

#[cfg(feature = "sdl")]
pub fn convert_sdl_mods_to_dakota(keymods: sdl2::keyboard::Mod) -> Mods {
    Mods::from_bits(keymods.bits()).expect("Invalid mod bits")
}

/// Keycodes for mouse buttons.
///
/// Names are self explanitory, `LEFT` for left click and etc.
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    UNKNOWN = 0,
    LEFT,
    MIDDLE,
    RIGHT,
    EXTRA,
    SIDE,
    BUTTON6,
    BUTTON7,
    BUTTON8,
}

/// Converts a Linux kernel mouse button code into a Dakota enum.
///
/// The conversion values are based on Linux's input.h
#[cfg(feature = "direct2display")]
pub fn convert_libinput_mouse_to_dakota(button: u32) -> MouseButton {
    match button {
        0x110 => MouseButton::LEFT,
        0x112 => MouseButton::MIDDLE,
        0x111 => MouseButton::RIGHT,
        0x114 => MouseButton::EXTRA,
        0x115 => MouseButton::SIDE,
        0x106 => MouseButton::BUTTON6,
        0x107 => MouseButton::BUTTON7,
        0x108 => MouseButton::BUTTON8,
        _ => MouseButton::UNKNOWN,
    }
}

#[cfg(feature = "sdl")]
pub fn convert_sdl_mouse_to_dakota(button: sdl2::mouse::MouseButton) -> MouseButton {
    match button {
        sdl2::mouse::MouseButton::Left => MouseButton::LEFT,
        sdl2::mouse::MouseButton::Middle => MouseButton::MIDDLE,
        sdl2::mouse::MouseButton::Right => MouseButton::RIGHT,
        sdl2::mouse::MouseButton::X1 => MouseButton::SIDE,
        sdl2::mouse::MouseButton::X2 => MouseButton::EXTRA,
        _ => MouseButton::UNKNOWN,
    }
}

/// Generic table for Keycode translations
///
/// We have three different sets of keycodes to translate between: Dakota::Keycode,
/// SDL2's Keycode, and xkbcommon's keycode. There are also other codes, such as mouse
/// buttons and such that also need to be translated.
///
/// This provides a generic way of defining a table of (Keycode, foreign Keycode) pairs
/// that we can translate between, in either direction.
struct CodeTranslator<K: PartialEq + Copy, T: PartialEq + Copy> {
    ct_table: Vec<(K, T)>,
}

impl<K: PartialEq + Copy, T: PartialEq + Copy> CodeTranslator<K, T> {
    /// Convert a foreign Keycode into a Dakota Keycode
    fn val_to_key(&self, val: T) -> Option<K> {
        for entry in self.ct_table.iter() {
            match entry.1 == val {
                true => return Some(entry.0),
                false => continue,
            }
        }

        return None;
    }

    /// Convert a Dakota Keycode into a foreign Keycode
    #[allow(dead_code)]
    fn key_to_val(&self, code: K) -> Option<T> {
        for entry in self.ct_table.iter() {
            match entry.0 == code {
                true => return Some(entry.1),
                false => continue,
            }
        }

        return None;
    }
}

// Define different tables for our different possible keycode sets
#[cfg(feature = "direct2display")]
lazy_static::lazy_static! {
    static ref CT_XKB_TO_DAKOTA: CodeTranslator<Keycode, u32> =
        CodeTranslator {
            ct_table: vec![
                (Keycode::RETURN,              xkb::keysyms::KEY_Return),
                (Keycode::ESCAPE,              xkb::keysyms::KEY_Escape),
                (Keycode::BACKSPACE,           xkb::keysyms::KEY_BackSpace),
                (Keycode::TAB,                 xkb::keysyms::KEY_Tab),
                (Keycode::SPACE,               xkb::keysyms::KEY_space),
                (Keycode::EXCLAIM,             xkb::keysyms::KEY_exclam),
                (Keycode::QUOTEDBL,            xkb::keysyms::KEY_quotedbl),
                (Keycode::HASH,                xkb::keysyms::KEY_sterling),
                (Keycode::PERCENT,             xkb::keysyms::KEY_percent),
                (Keycode::DOLLAR,              xkb::keysyms::KEY_dollar),
                (Keycode::AMPERSAND,           xkb::keysyms::KEY_ampersand),
                (Keycode::QUOTE,               xkb::keysyms::KEY_quoteleft),
                (Keycode::QUOTE,               xkb::keysyms::KEY_quoteright),
                (Keycode::LEFTPAREN,           xkb::keysyms::KEY_parenleft),
                (Keycode::RIGHTPAREN,          xkb::keysyms::KEY_parenright),
                (Keycode::ASTERISK,            xkb::keysyms::KEY_asterisk),
                (Keycode::PLUS,                xkb::keysyms::KEY_plus),
                (Keycode::COMMA,               xkb::keysyms::KEY_comma),
                (Keycode::MINUS,               xkb::keysyms::KEY_minus),
                (Keycode::PERIOD,              xkb::keysyms::KEY_period),
                (Keycode::SLASH,               xkb::keysyms::KEY_slash),
                (Keycode::NUM0,                xkb::keysyms::KEY_0),
                (Keycode::NUM1,                xkb::keysyms::KEY_1),
                (Keycode::NUM2,                xkb::keysyms::KEY_2),
                (Keycode::NUM3,                xkb::keysyms::KEY_3),
                (Keycode::NUM4,                xkb::keysyms::KEY_4),
                (Keycode::NUM5,                xkb::keysyms::KEY_5),
                (Keycode::NUM6,                xkb::keysyms::KEY_6),
                (Keycode::NUM7,                xkb::keysyms::KEY_7),
                (Keycode::NUM8,                xkb::keysyms::KEY_8),
                (Keycode::NUM9,                xkb::keysyms::KEY_9),
                (Keycode::COLON,               xkb::keysyms::KEY_colon),
                (Keycode::SEMICOLON,           xkb::keysyms::KEY_semicolon),
                (Keycode::LESS,                xkb::keysyms::KEY_less),
                (Keycode::EQUALS,              xkb::keysyms::KEY_equal),
                (Keycode::GREATER,             xkb::keysyms::KEY_greater),
                (Keycode::QUESTION,            xkb::keysyms::KEY_question),
                (Keycode::AT,                  xkb::keysyms::KEY_at),
                (Keycode::LEFTBRACKET,         xkb::keysyms::KEY_bracketleft),
                (Keycode::BACKSLASH,           xkb::keysyms::KEY_backslash),
                (Keycode::RIGHTBRACKET,        xkb::keysyms::KEY_bracketright),
                (Keycode::CARET,               xkb::keysyms::KEY_caret),
                (Keycode::UNDERSCORE,          xkb::keysyms::KEY_underscore),
                // Two copies of letters, upper and lowercase
                (Keycode::A,                   xkb::keysyms::KEY_A),
                (Keycode::B,                   xkb::keysyms::KEY_B),
                (Keycode::C,                   xkb::keysyms::KEY_C),
                (Keycode::D,                   xkb::keysyms::KEY_D),
                (Keycode::E,                   xkb::keysyms::KEY_E),
                (Keycode::F,                   xkb::keysyms::KEY_F),
                (Keycode::G,                   xkb::keysyms::KEY_G),
                (Keycode::H,                   xkb::keysyms::KEY_H),
                (Keycode::I,                   xkb::keysyms::KEY_I),
                (Keycode::J,                   xkb::keysyms::KEY_J),
                (Keycode::K,                   xkb::keysyms::KEY_K),
                (Keycode::L,                   xkb::keysyms::KEY_L),
                (Keycode::M,                   xkb::keysyms::KEY_M),
                (Keycode::N,                   xkb::keysyms::KEY_N),
                (Keycode::O,                   xkb::keysyms::KEY_O),
                (Keycode::P,                   xkb::keysyms::KEY_P),
                (Keycode::Q,                   xkb::keysyms::KEY_Q),
                (Keycode::R,                   xkb::keysyms::KEY_R),
                (Keycode::S,                   xkb::keysyms::KEY_S),
                (Keycode::T,                   xkb::keysyms::KEY_T),
                (Keycode::U,                   xkb::keysyms::KEY_U),
                (Keycode::V,                   xkb::keysyms::KEY_V),
                (Keycode::W,                   xkb::keysyms::KEY_W),
                (Keycode::X,                   xkb::keysyms::KEY_X),
                (Keycode::Y,                   xkb::keysyms::KEY_Y),
                (Keycode::Z,                   xkb::keysyms::KEY_Z),
                (Keycode::A,                   xkb::keysyms::KEY_a),
                (Keycode::B,                   xkb::keysyms::KEY_b),
                (Keycode::C,                   xkb::keysyms::KEY_c),
                (Keycode::D,                   xkb::keysyms::KEY_d),
                (Keycode::E,                   xkb::keysyms::KEY_e),
                (Keycode::F,                   xkb::keysyms::KEY_f),
                (Keycode::G,                   xkb::keysyms::KEY_g),
                (Keycode::H,                   xkb::keysyms::KEY_h),
                (Keycode::I,                   xkb::keysyms::KEY_i),
                (Keycode::J,                   xkb::keysyms::KEY_j),
                (Keycode::K,                   xkb::keysyms::KEY_k),
                (Keycode::L,                   xkb::keysyms::KEY_l),
                (Keycode::M,                   xkb::keysyms::KEY_m),
                (Keycode::N,                   xkb::keysyms::KEY_n),
                (Keycode::O,                   xkb::keysyms::KEY_o),
                (Keycode::P,                   xkb::keysyms::KEY_p),
                (Keycode::Q,                   xkb::keysyms::KEY_q),
                (Keycode::R,                   xkb::keysyms::KEY_r),
                (Keycode::S,                   xkb::keysyms::KEY_s),
                (Keycode::T,                   xkb::keysyms::KEY_t),
                (Keycode::U,                   xkb::keysyms::KEY_u),
                (Keycode::V,                   xkb::keysyms::KEY_v),
                (Keycode::W,                   xkb::keysyms::KEY_w),
                (Keycode::X,                   xkb::keysyms::KEY_x),
                (Keycode::Y,                   xkb::keysyms::KEY_y),
                (Keycode::Z,                   xkb::keysyms::KEY_z),
                (Keycode::CAPSLOCK,            xkb::keysyms::KEY_Caps_Lock),
                (Keycode::F1,                  xkb::keysyms::KEY_F1),
                (Keycode::F2,                  xkb::keysyms::KEY_F2),
                (Keycode::F3,                  xkb::keysyms::KEY_F3),
                (Keycode::F4,                  xkb::keysyms::KEY_F4),
                (Keycode::F5,                  xkb::keysyms::KEY_F5),
                (Keycode::F6,                  xkb::keysyms::KEY_F6),
                (Keycode::F7,                  xkb::keysyms::KEY_F7),
                (Keycode::F8,                  xkb::keysyms::KEY_F8),
                (Keycode::F9,                  xkb::keysyms::KEY_F9),
                (Keycode::F10,                 xkb::keysyms::KEY_F10),
                (Keycode::F11,                 xkb::keysyms::KEY_F11),
                (Keycode::F12,                 xkb::keysyms::KEY_F12),
                (Keycode::PRINTSCREEN,         xkb::keysyms::KEY_Print),
                (Keycode::SCROLLLOCK,          xkb::keysyms::KEY_Scroll_Lock),
                (Keycode::PAUSE,               xkb::keysyms::KEY_Pause),
                (Keycode::INSERT,              xkb::keysyms::KEY_Insert),
                (Keycode::HOME,                xkb::keysyms::KEY_Home),
                (Keycode::PAGEUP,              xkb::keysyms::KEY_Page_Up),
                (Keycode::DELETE,              xkb::keysyms::KEY_Delete),
                (Keycode::END,                 xkb::keysyms::KEY_End),
                (Keycode::PAGEDOWN,            xkb::keysyms::KEY_Page_Down),
                (Keycode::RIGHT,               xkb::keysyms::KEY_Right),
                (Keycode::LEFT,                xkb::keysyms::KEY_Left),
                (Keycode::DOWN,                xkb::keysyms::KEY_Down),
                (Keycode::UP,                  xkb::keysyms::KEY_Up),
                (Keycode::NUMLOCK,             xkb::keysyms::KEY_Num_Lock),
                (Keycode::KP_DIVIDE,           xkb::keysyms::KEY_KP_Divide),
                (Keycode::KP_MULTIPLY,         xkb::keysyms::KEY_KP_Multiply),
                (Keycode::KP_ENTER,            xkb::keysyms::KEY_KP_Enter),
                (Keycode::KP_1,                xkb::keysyms::KEY_KP_1),
                (Keycode::KP_2,                xkb::keysyms::KEY_KP_2),
                (Keycode::KP_3,                xkb::keysyms::KEY_KP_3),
                (Keycode::KP_4,                xkb::keysyms::KEY_KP_4),
                (Keycode::KP_5,                xkb::keysyms::KEY_KP_5),
                (Keycode::KP_6,                xkb::keysyms::KEY_KP_6),
                (Keycode::KP_7,                xkb::keysyms::KEY_KP_7),
                (Keycode::KP_8,                xkb::keysyms::KEY_KP_8),
                (Keycode::KP_9,                xkb::keysyms::KEY_KP_9),
                (Keycode::KP_0,                xkb::keysyms::KEY_KP_0),
                (Keycode::APPLICATION,         xkb::keysyms::KEY_XF86ApplicationLeft),
                (Keycode::APPLICATION,         xkb::keysyms::KEY_XF86ApplicationRight),
                (Keycode::POWER,               xkb::keysyms::KEY_XF86PowerOff),
                (Keycode::KP_EQUALS,           xkb::keysyms::KEY_KP_Equal),
                (Keycode::F13,                 xkb::keysyms::KEY_F13),
                (Keycode::F14,                 xkb::keysyms::KEY_F14),
                (Keycode::F15,                 xkb::keysyms::KEY_F15),
                (Keycode::F16,                 xkb::keysyms::KEY_F16),
                (Keycode::F17,                 xkb::keysyms::KEY_F17),
                (Keycode::F18,                 xkb::keysyms::KEY_F18),
                (Keycode::F19,                 xkb::keysyms::KEY_F19),
                (Keycode::F20,                 xkb::keysyms::KEY_F20),
                (Keycode::F21,                 xkb::keysyms::KEY_F21),
                (Keycode::F22,                 xkb::keysyms::KEY_F22),
                (Keycode::F23,                 xkb::keysyms::KEY_F23),
                (Keycode::F24,                 xkb::keysyms::KEY_F24),
                (Keycode::EXECUTE,             xkb::keysyms::KEY_Execute),
                (Keycode::HELP,                xkb::keysyms::KEY_Help),
                (Keycode::MENU,                xkb::keysyms::KEY_Menu),
                (Keycode::SELECT,              xkb::keysyms::KEY_Select),
                (Keycode::STOP,                xkb::keysyms::KEY_XF86Stop),
                (Keycode::REDO,                xkb::keysyms::KEY_Redo),
                (Keycode::UNDO,                xkb::keysyms::KEY_Undo),
                (Keycode::CUT,                 xkb::keysyms::KEY_XF86Cut),
                (Keycode::COPY,                xkb::keysyms::KEY_XF86Copy),
                (Keycode::PASTE,               xkb::keysyms::KEY_XF86Paste),
                (Keycode::FIND,                xkb::keysyms::KEY_Find),
                (Keycode::MUTE,                xkb::keysyms::KEY_XF86AudioMute),
                (Keycode::VOLUMEUP,            xkb::keysyms::KEY_XF86AudioLowerVolume),
                (Keycode::VOLUMEDOWN,          xkb::keysyms::KEY_XF86AudioRaiseVolume),
                (Keycode::SYSREQ,              xkb::keysyms::KEY_Sys_Req),
                (Keycode::CANCEL,              xkb::keysyms::KEY_Cancel),
                (Keycode::CLEAR,               xkb::keysyms::KEY_Clear),
                (Keycode::PRIOR,               xkb::keysyms::KEY_Prior),
                (Keycode::SEPARATOR,           xkb::keysyms::KEY_KP_Separator),
                (Keycode::CURRENCYUNIT,        xkb::keysyms::KEY_currency),
                (Keycode::KP_TAB,              xkb::keysyms::KEY_KP_Tab),
                (Keycode::VERTICALBAR,         xkb::keysyms::KEY_vertbar),
                (Keycode::KP_SPACE,            xkb::keysyms::KEY_KP_Space),
                (Keycode::KP_DECIMAL,          xkb::keysyms::KEY_KP_Decimal),
                (Keycode::LCTRL,               xkb::keysyms::KEY_Control_L),
                (Keycode::LSHIFT,              xkb::keysyms::KEY_Shift_L),
                (Keycode::LALT,                xkb::keysyms::KEY_Alt_L),
                (Keycode::LGUI,                xkb::keysyms::KEY_Super_L),
                (Keycode::RCTRL,               xkb::keysyms::KEY_Control_R),
                (Keycode::RSHIFT,              xkb::keysyms::KEY_Shift_R),
                (Keycode::RALT,                xkb::keysyms::KEY_Alt_R),
                (Keycode::RGUI,                xkb::keysyms::KEY_Super_R),
                (Keycode::AUDIONEXT,           xkb::keysyms::KEY_XF86AudioNext),
                (Keycode::AUDIOPREV,           xkb::keysyms::KEY_XF86AudioPrev),
                (Keycode::AUDIOSTOP,           xkb::keysyms::KEY_XF86AudioStop),
                (Keycode::AUDIOPLAY,           xkb::keysyms::KEY_XF86AudioPlay),
                (Keycode::AUDIOMUTE,           xkb::keysyms::KEY_XF86AudioMute),
                (Keycode::WWW,                 xkb::keysyms::KEY_XF86WWW),
                (Keycode::MAIL,                xkb::keysyms::KEY_XF86Mail),
                (Keycode::CALCULATOR,          xkb::keysyms::KEY_XF86Calculator),
                (Keycode::BRIGHTNESSDOWN,      xkb::keysyms::KEY_XF86MonBrightnessDown),
                (Keycode::BRIGHTNESSUP,        xkb::keysyms::KEY_XF86MonBrightnessUp),
                (Keycode::DISPLAYSWITCH,       xkb::keysyms::KEY_XF86Display),
                (Keycode::KBDILLUMTOGGLE,      xkb::keysyms::KEY_XF86KbdLightOnOff),
                (Keycode::KBDILLUMDOWN,        xkb::keysyms::KEY_XF86KbdBrightnessDown),
                (Keycode::KBDILLUMUP,          xkb::keysyms::KEY_XF86KbdBrightnessUp),
                (Keycode::EJECT,               xkb::keysyms::KEY_XF86Eject),
                (Keycode::SLEEP,               xkb::keysyms::KEY_XF86Sleep),
            ]
        };
}

#[cfg(feature = "sdl")]
lazy_static::lazy_static! {
    static ref CT_SDL_TO_DAKOTA: CodeTranslator<Keycode, sdl2::keyboard::Keycode> =
        CodeTranslator {
            ct_table: vec![
                (Keycode::RETURN,              sdl2::keyboard::Keycode::Return),
                (Keycode::ESCAPE,              sdl2::keyboard::Keycode::Escape),
                (Keycode::BACKSPACE,           sdl2::keyboard::Keycode::Backspace),
                (Keycode::TAB,                 sdl2::keyboard::Keycode::Tab),
                (Keycode::SPACE,               sdl2::keyboard::Keycode::Space),
                (Keycode::EXCLAIM,             sdl2::keyboard::Keycode::Exclaim),
                (Keycode::QUOTEDBL,            sdl2::keyboard::Keycode::Quotedbl),
                (Keycode::HASH,                sdl2::keyboard::Keycode::Hash),
                (Keycode::PERCENT,             sdl2::keyboard::Keycode::Percent),
                (Keycode::DOLLAR,              sdl2::keyboard::Keycode::Dollar),
                (Keycode::AMPERSAND,           sdl2::keyboard::Keycode::Ampersand),
                (Keycode::QUOTE,               sdl2::keyboard::Keycode::Quote),
                (Keycode::LEFTPAREN,           sdl2::keyboard::Keycode::LeftParen),
                (Keycode::RIGHTPAREN,          sdl2::keyboard::Keycode::RightParen),
                (Keycode::ASTERISK,            sdl2::keyboard::Keycode::Asterisk),
                (Keycode::PLUS,                sdl2::keyboard::Keycode::Plus),
                (Keycode::COMMA,               sdl2::keyboard::Keycode::Comma),
                (Keycode::MINUS,               sdl2::keyboard::Keycode::Minus),
                (Keycode::PERIOD,              sdl2::keyboard::Keycode::Period),
                (Keycode::SLASH,               sdl2::keyboard::Keycode::Slash),
                (Keycode::NUM0,                sdl2::keyboard::Keycode::Num0),
                (Keycode::NUM1,                sdl2::keyboard::Keycode::Num1),
                (Keycode::NUM2,                sdl2::keyboard::Keycode::Num2),
                (Keycode::NUM3,                sdl2::keyboard::Keycode::Num3),
                (Keycode::NUM4,                sdl2::keyboard::Keycode::Num4),
                (Keycode::NUM5,                sdl2::keyboard::Keycode::Num5),
                (Keycode::NUM6,                sdl2::keyboard::Keycode::Num6),
                (Keycode::NUM7,                sdl2::keyboard::Keycode::Num7),
                (Keycode::NUM8,                sdl2::keyboard::Keycode::Num8),
                (Keycode::NUM9,                sdl2::keyboard::Keycode::Num9),
                (Keycode::COLON,               sdl2::keyboard::Keycode::Colon),
                (Keycode::SEMICOLON,           sdl2::keyboard::Keycode::Semicolon),
                (Keycode::LESS,                sdl2::keyboard::Keycode::Less),
                (Keycode::EQUALS,              sdl2::keyboard::Keycode::Equals),
                (Keycode::GREATER,             sdl2::keyboard::Keycode::Greater),
                (Keycode::QUESTION,            sdl2::keyboard::Keycode::Question),
                (Keycode::AT,                  sdl2::keyboard::Keycode::At),
                (Keycode::LEFTBRACKET,         sdl2::keyboard::Keycode::LeftBracket),
                (Keycode::BACKSLASH,           sdl2::keyboard::Keycode::Backslash),
                (Keycode::RIGHTBRACKET,        sdl2::keyboard::Keycode::RightBracket),
                (Keycode::CARET,               sdl2::keyboard::Keycode::Caret),
                (Keycode::UNDERSCORE,          sdl2::keyboard::Keycode::Underscore),
                (Keycode::BACKQUOTE,           sdl2::keyboard::Keycode::Backquote),
                (Keycode::A,                   sdl2::keyboard::Keycode::A),
                (Keycode::B,                   sdl2::keyboard::Keycode::B),
                (Keycode::C,                   sdl2::keyboard::Keycode::C),
                (Keycode::D,                   sdl2::keyboard::Keycode::D),
                (Keycode::E,                   sdl2::keyboard::Keycode::E),
                (Keycode::F,                   sdl2::keyboard::Keycode::F),
                (Keycode::G,                   sdl2::keyboard::Keycode::G),
                (Keycode::H,                   sdl2::keyboard::Keycode::H),
                (Keycode::I,                   sdl2::keyboard::Keycode::I),
                (Keycode::J,                   sdl2::keyboard::Keycode::J),
                (Keycode::K,                   sdl2::keyboard::Keycode::K),
                (Keycode::L,                   sdl2::keyboard::Keycode::L),
                (Keycode::M,                   sdl2::keyboard::Keycode::M),
                (Keycode::N,                   sdl2::keyboard::Keycode::N),
                (Keycode::O,                   sdl2::keyboard::Keycode::O),
                (Keycode::P,                   sdl2::keyboard::Keycode::P),
                (Keycode::Q,                   sdl2::keyboard::Keycode::Q),
                (Keycode::R,                   sdl2::keyboard::Keycode::R),
                (Keycode::S,                   sdl2::keyboard::Keycode::S),
                (Keycode::T,                   sdl2::keyboard::Keycode::T),
                (Keycode::U,                   sdl2::keyboard::Keycode::U),
                (Keycode::V,                   sdl2::keyboard::Keycode::V),
                (Keycode::W,                   sdl2::keyboard::Keycode::W),
                (Keycode::X,                   sdl2::keyboard::Keycode::X),
                (Keycode::Y,                   sdl2::keyboard::Keycode::Y),
                (Keycode::Z,                   sdl2::keyboard::Keycode::Z),
                (Keycode::CAPSLOCK,            sdl2::keyboard::Keycode::CapsLock),
                (Keycode::F1,                  sdl2::keyboard::Keycode::F1),
                (Keycode::F2,                  sdl2::keyboard::Keycode::F2),
                (Keycode::F3,                  sdl2::keyboard::Keycode::F3),
                (Keycode::F4,                  sdl2::keyboard::Keycode::F4),
                (Keycode::F5,                  sdl2::keyboard::Keycode::F5),
                (Keycode::F6,                  sdl2::keyboard::Keycode::F6),
                (Keycode::F7,                  sdl2::keyboard::Keycode::F7),
                (Keycode::F8,                  sdl2::keyboard::Keycode::F8),
                (Keycode::F9,                  sdl2::keyboard::Keycode::F9),
                (Keycode::F10,                 sdl2::keyboard::Keycode::F10),
                (Keycode::F11,                 sdl2::keyboard::Keycode::F11),
                (Keycode::F12,                 sdl2::keyboard::Keycode::F12),
                (Keycode::PRINTSCREEN,         sdl2::keyboard::Keycode::PrintScreen),
                (Keycode::SCROLLLOCK,          sdl2::keyboard::Keycode::ScrollLock),
                (Keycode::PAUSE,               sdl2::keyboard::Keycode::Pause),
                (Keycode::INSERT,              sdl2::keyboard::Keycode::Insert),
                (Keycode::HOME,                sdl2::keyboard::Keycode::Home),
                (Keycode::PAGEUP,              sdl2::keyboard::Keycode::PageUp),
                (Keycode::DELETE,              sdl2::keyboard::Keycode::Delete),
                (Keycode::END,                 sdl2::keyboard::Keycode::End),
                (Keycode::PAGEDOWN,            sdl2::keyboard::Keycode::PageDown),
                (Keycode::RIGHT,               sdl2::keyboard::Keycode::Right),
                (Keycode::LEFT,                sdl2::keyboard::Keycode::Left),
                (Keycode::DOWN,                sdl2::keyboard::Keycode::Down),
                (Keycode::UP,                  sdl2::keyboard::Keycode::Up),
                (Keycode::NUMLOCK,             sdl2::keyboard::Keycode::NumLockClear),
                (Keycode::KP_DIVIDE,           sdl2::keyboard::Keycode::KpDivide),
                (Keycode::KP_MULTIPLY,         sdl2::keyboard::Keycode::KpMultiply),
                (Keycode::KP_MINUS,            sdl2::keyboard::Keycode::KpMinus),
                (Keycode::KP_PLUS,             sdl2::keyboard::Keycode::KpPlus),
                (Keycode::KP_ENTER,            sdl2::keyboard::Keycode::KpEnter),
                (Keycode::KP_1,                sdl2::keyboard::Keycode::Kp1),
                (Keycode::KP_2,                sdl2::keyboard::Keycode::Kp2),
                (Keycode::KP_3,                sdl2::keyboard::Keycode::Kp3),
                (Keycode::KP_4,                sdl2::keyboard::Keycode::Kp4),
                (Keycode::KP_5,                sdl2::keyboard::Keycode::Kp5),
                (Keycode::KP_6,                sdl2::keyboard::Keycode::Kp6),
                (Keycode::KP_7,                sdl2::keyboard::Keycode::Kp7),
                (Keycode::KP_8,                sdl2::keyboard::Keycode::Kp8),
                (Keycode::KP_9,                sdl2::keyboard::Keycode::Kp9),
                (Keycode::KP_0,                sdl2::keyboard::Keycode::Kp0),
                (Keycode::KP_PERIOD,           sdl2::keyboard::Keycode::KpPeriod),
                (Keycode::APPLICATION,         sdl2::keyboard::Keycode::Application),
                (Keycode::POWER,               sdl2::keyboard::Keycode::Power),
                (Keycode::KP_EQUALS,           sdl2::keyboard::Keycode::KpEquals),
                (Keycode::F13,                 sdl2::keyboard::Keycode::F13),
                (Keycode::F14,                 sdl2::keyboard::Keycode::F14),
                (Keycode::F15,                 sdl2::keyboard::Keycode::F15),
                (Keycode::F16,                 sdl2::keyboard::Keycode::F16),
                (Keycode::F17,                 sdl2::keyboard::Keycode::F17),
                (Keycode::F18,                 sdl2::keyboard::Keycode::F18),
                (Keycode::F19,                 sdl2::keyboard::Keycode::F19),
                (Keycode::F20,                 sdl2::keyboard::Keycode::F20),
                (Keycode::F21,                 sdl2::keyboard::Keycode::F21),
                (Keycode::F22,                 sdl2::keyboard::Keycode::F22),
                (Keycode::F23,                 sdl2::keyboard::Keycode::F23),
                (Keycode::F24,                 sdl2::keyboard::Keycode::F24),
                (Keycode::EXECUTE,             sdl2::keyboard::Keycode::Execute),
                (Keycode::HELP,                sdl2::keyboard::Keycode::Help),
                (Keycode::MENU,                sdl2::keyboard::Keycode::Menu),
                (Keycode::SELECT,              sdl2::keyboard::Keycode::Select),
                (Keycode::STOP,                sdl2::keyboard::Keycode::Stop),
                (Keycode::REDO,                sdl2::keyboard::Keycode::Again),
                (Keycode::UNDO,                sdl2::keyboard::Keycode::Undo),
                (Keycode::CUT,                 sdl2::keyboard::Keycode::Cut),
                (Keycode::COPY,                sdl2::keyboard::Keycode::Copy),
                (Keycode::PASTE,               sdl2::keyboard::Keycode::Paste),
                (Keycode::FIND,                sdl2::keyboard::Keycode::Find),
                (Keycode::MUTE,                sdl2::keyboard::Keycode::Mute),
                (Keycode::VOLUMEUP,            sdl2::keyboard::Keycode::VolumeUp),
                (Keycode::VOLUMEDOWN,          sdl2::keyboard::Keycode::VolumeDown),
                (Keycode::KP_COMMA,            sdl2::keyboard::Keycode::KpComma),
                (Keycode::KP_EQUALSAS400,      sdl2::keyboard::Keycode::KpEqualsAS400),
                (Keycode::ALTERASE,            sdl2::keyboard::Keycode::AltErase),
                (Keycode::SYSREQ,              sdl2::keyboard::Keycode::Sysreq),
                (Keycode::CANCEL,              sdl2::keyboard::Keycode::Cancel),
                (Keycode::CLEAR,               sdl2::keyboard::Keycode::Clear),
                (Keycode::PRIOR,               sdl2::keyboard::Keycode::Prior),
                (Keycode::RETURN2,             sdl2::keyboard::Keycode::Return2),
                (Keycode::SEPARATOR,           sdl2::keyboard::Keycode::Separator),
                (Keycode::OUT,                 sdl2::keyboard::Keycode::Out),
                (Keycode::OPER,                sdl2::keyboard::Keycode::Oper),
                (Keycode::CLEARAGAIN,          sdl2::keyboard::Keycode::ClearAgain),
                (Keycode::CRSEL,               sdl2::keyboard::Keycode::CrSel),
                (Keycode::EXSEL,               sdl2::keyboard::Keycode::ExSel),
                (Keycode::KP_00,               sdl2::keyboard::Keycode::Kp00),
                (Keycode::KP_000,              sdl2::keyboard::Keycode::Kp000),
                (Keycode::THOUSANDSSEPARATOR,  sdl2::keyboard::Keycode::ThousandsSeparator),
                (Keycode::DECIMALSEPARATOR,    sdl2::keyboard::Keycode::DecimalSeparator),
                (Keycode::CURRENCYUNIT,        sdl2::keyboard::Keycode::CurrencyUnit),
                (Keycode::CURRENCYSUBUNIT,     sdl2::keyboard::Keycode::CurrencySubUnit),
                (Keycode::KP_LEFTPAREN,        sdl2::keyboard::Keycode::KpLeftParen),
                (Keycode::KP_RIGHTPAREN,       sdl2::keyboard::Keycode::KpRightParen),
                (Keycode::KP_LEFTBRACE,        sdl2::keyboard::Keycode::KpLeftBrace),
                (Keycode::KP_RIGHTBRACE,       sdl2::keyboard::Keycode::KpRightBrace),
                (Keycode::KP_TAB,              sdl2::keyboard::Keycode::KpTab),
                (Keycode::KP_BACKSPACE,        sdl2::keyboard::Keycode::KpBackspace),
                (Keycode::KP_A,                sdl2::keyboard::Keycode::KpA),
                (Keycode::KP_B,                sdl2::keyboard::Keycode::KpB),
                (Keycode::KP_C,                sdl2::keyboard::Keycode::KpC),
                (Keycode::KP_D,                sdl2::keyboard::Keycode::KpD),
                (Keycode::KP_E,                sdl2::keyboard::Keycode::KpE),
                (Keycode::KP_F,                sdl2::keyboard::Keycode::KpF),
                (Keycode::KP_XOR,              sdl2::keyboard::Keycode::KpXor),
                (Keycode::KP_POWER,            sdl2::keyboard::Keycode::KpPower),
                (Keycode::KP_PERCENT,          sdl2::keyboard::Keycode::KpPercent),
                (Keycode::KP_LESS,             sdl2::keyboard::Keycode::KpLess),
                (Keycode::KP_GREATER,          sdl2::keyboard::Keycode::KpGreater),
                (Keycode::KP_AMPERSAND,        sdl2::keyboard::Keycode::KpAmpersand),
                (Keycode::KP_DBLAMPERSAND,     sdl2::keyboard::Keycode::KpDblAmpersand),
                (Keycode::KP_VERTICALBAR,      sdl2::keyboard::Keycode::KpVerticalBar),
                (Keycode::KP_DBLVERTICALBAR,   sdl2::keyboard::Keycode::KpDblVerticalBar),
                (Keycode::KP_COLON,            sdl2::keyboard::Keycode::KpColon),
                (Keycode::KP_HASH,             sdl2::keyboard::Keycode::KpHash),
                (Keycode::KP_SPACE,            sdl2::keyboard::Keycode::KpSpace),
                (Keycode::KP_AT,               sdl2::keyboard::Keycode::KpAt),
                (Keycode::KP_EXCLAM,           sdl2::keyboard::Keycode::KpExclam),
                (Keycode::KP_MEMSTORE,         sdl2::keyboard::Keycode::KpMemStore),
                (Keycode::KP_MEMRECALL,        sdl2::keyboard::Keycode::KpMemRecall),
                (Keycode::KP_MEMCLEAR,         sdl2::keyboard::Keycode::KpMemClear),
                (Keycode::KP_MEMADD,           sdl2::keyboard::Keycode::KpMemAdd),
                (Keycode::KP_MEMSUBTRACT,      sdl2::keyboard::Keycode::KpMemSubtract),
                (Keycode::KP_MEMMULTIPLY,      sdl2::keyboard::Keycode::KpMemMultiply),
                (Keycode::KP_MEMDIVIDE,        sdl2::keyboard::Keycode::KpMemDivide),
                (Keycode::KP_PLUSMINUS,        sdl2::keyboard::Keycode::KpPlusMinus),
                (Keycode::KP_CLEAR,            sdl2::keyboard::Keycode::KpClear),
                (Keycode::KP_CLEARENTRY,       sdl2::keyboard::Keycode::KpClearEntry),
                (Keycode::KP_BINARY,           sdl2::keyboard::Keycode::KpBinary),
                (Keycode::KP_OCTAL,            sdl2::keyboard::Keycode::KpOctal),
                (Keycode::KP_DECIMAL,          sdl2::keyboard::Keycode::KpDecimal),
                (Keycode::KP_HEXADECIMAL,      sdl2::keyboard::Keycode::KpHexadecimal),
                (Keycode::LCTRL,               sdl2::keyboard::Keycode::LCtrl),
                (Keycode::LSHIFT,              sdl2::keyboard::Keycode::LShift),
                (Keycode::LALT,                sdl2::keyboard::Keycode::LAlt),
                (Keycode::LGUI,                sdl2::keyboard::Keycode::LGui),
                (Keycode::RCTRL,               sdl2::keyboard::Keycode::RCtrl),
                (Keycode::RSHIFT,              sdl2::keyboard::Keycode::RShift),
                (Keycode::RALT,                sdl2::keyboard::Keycode::RAlt),
                (Keycode::RGUI,                sdl2::keyboard::Keycode::RGui),
                (Keycode::MODE,                sdl2::keyboard::Keycode::Mode),
                (Keycode::AUDIONEXT,           sdl2::keyboard::Keycode::AudioNext),
                (Keycode::AUDIOPREV,           sdl2::keyboard::Keycode::AudioPrev),
                (Keycode::AUDIOSTOP,           sdl2::keyboard::Keycode::AudioStop),
                (Keycode::AUDIOPLAY,           sdl2::keyboard::Keycode::AudioPlay),
                (Keycode::AUDIOMUTE,           sdl2::keyboard::Keycode::AudioMute),
                (Keycode::MEDIASELECT,         sdl2::keyboard::Keycode::MediaSelect),
                (Keycode::WWW,                 sdl2::keyboard::Keycode::Www),
                (Keycode::MAIL,                sdl2::keyboard::Keycode::Mail),
                (Keycode::CALCULATOR,          sdl2::keyboard::Keycode::Calculator),
                (Keycode::COMPUTER,            sdl2::keyboard::Keycode::Computer),
                (Keycode::AC_SEARCH,           sdl2::keyboard::Keycode::AcSearch),
                (Keycode::AC_HOME,             sdl2::keyboard::Keycode::AcHome),
                (Keycode::AC_BACK,             sdl2::keyboard::Keycode::AcBack),
                (Keycode::AC_FORWARD,          sdl2::keyboard::Keycode::AcForward),
                (Keycode::AC_STOP,             sdl2::keyboard::Keycode::AcStop),
                (Keycode::AC_REFRESH,          sdl2::keyboard::Keycode::AcRefresh),
                (Keycode::AC_BOOKMARKS,        sdl2::keyboard::Keycode::AcBookmarks),
                (Keycode::BRIGHTNESSDOWN,      sdl2::keyboard::Keycode::BrightnessDown),
                (Keycode::BRIGHTNESSUP,        sdl2::keyboard::Keycode::BrightnessUp),
                (Keycode::DISPLAYSWITCH,       sdl2::keyboard::Keycode::DisplaySwitch),
                (Keycode::KBDILLUMTOGGLE,      sdl2::keyboard::Keycode::KbdIllumToggle),
                (Keycode::KBDILLUMDOWN,        sdl2::keyboard::Keycode::KbdIllumDown),
                (Keycode::KBDILLUMUP,          sdl2::keyboard::Keycode::KbdIllumUp),
                (Keycode::EJECT,               sdl2::keyboard::Keycode::Eject),
                (Keycode::SLEEP,               sdl2::keyboard::Keycode::Sleep),
                ],
        };
}

#[cfg(feature = "sdl")]
lazy_static::lazy_static! {
    static ref CT_SDL_TO_LINUX_KEY: CodeTranslator<u32, u32> =
        CodeTranslator {
            ct_table: vec![
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x000), // KEY_RESERVED
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_ESCAPE as u32,           0x001), // KEY_ESC
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_1 as u32,                0x002), // KEY_1
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_2 as u32,                0x003), // KEY_2
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_3 as u32,                0x004), // KEY_3
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_4 as u32,                0x005), // KEY_4
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_5 as u32,                0x006), // KEY_5
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_6 as u32,                0x007), // KEY_6
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_7 as u32,                0x008), // KEY_7
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_8 as u32,                0x009), // KEY_8
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_9 as u32,                0x00a), // KEY_9
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_0 as u32,                0x00b), // KEY_0
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_MINUS as u32,            0x00c), // KEY_MINUS
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_EQUALS as u32,           0x00d), // KEY_EQUAL
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_BACKSPACE as u32,        0x00e), // KEY_BACKSPACE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_TAB as u32,              0x00f), // KEY_TAB
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_Q as u32,                0x010), // KEY_Q
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_W as u32,                0x011), // KEY_W
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_E as u32,                0x012), // KEY_E
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_R as u32,                0x013), // KEY_R
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_T as u32,                0x014), // KEY_T
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_Y as u32,                0x015), // KEY_Y
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_U as u32,                0x016), // KEY_U
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_I as u32,                0x017), // KEY_I
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_O as u32,                0x018), // KEY_O
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_P as u32,                0x019), // KEY_P
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LEFTBRACKET as u32,      0x01a), // KEY_LEFTBRACE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_RIGHTBRACKET as u32,     0x01b), // KEY_RIGHTBRACE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_RETURN as u32,           0x01c), // KEY_ENTER
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LCTRL as u32,            0x01d), // KEY_LEFTCTRL
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_A as u32,                0x01e), // KEY_A
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_S as u32,                0x01f), // KEY_S
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_D as u32,                0x020), // KEY_D
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F as u32,                0x021), // KEY_F
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_G as u32,                0x022), // KEY_G
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_H as u32,                0x023), // KEY_H
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_J as u32,                0x024), // KEY_J
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_K as u32,                0x025), // KEY_K
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_L as u32,                0x026), // KEY_L
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_SEMICOLON as u32,        0x027), // KEY_SEMICOLON
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_APOSTROPHE as u32,       0x028), // KEY_APOSTROPHE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_GRAVE as u32,            0x029), // KEY_GRAVE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LSHIFT as u32,           0x02a), // KEY_LEFTSHIFT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_BACKSLASH as u32,        0x02b), // KEY_BACKSLASH
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_Z as u32,                0x02c), // KEY_Z
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_X as u32,                0x02d), // KEY_X
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_C as u32,                0x02e), // KEY_C
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_V as u32,                0x02f), // KEY_V
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_B as u32,                0x030), // KEY_B
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_N as u32,                0x031), // KEY_N
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_M as u32,                0x032), // KEY_M
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_COMMA as u32,            0x033), // KEY_COMMA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_PERIOD as u32,           0x034), // KEY_DOT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_SLASH as u32,            0x035), // KEY_SLASH
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_RSHIFT as u32,           0x036), // KEY_RIGHTSHIFT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_MULTIPLY as u32,      0x037), // KEY_KPASTERISK
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LALT as u32,             0x038), // KEY_LEFTALT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_SPACE as u32,            0x039), // KEY_SPACE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_CAPSLOCK as u32,         0x03a), // KEY_CAPSLOCK
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F1 as u32,               0x03b), // KEY_F1
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F2 as u32,               0x03c), // KEY_F2
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F3 as u32,               0x03d), // KEY_F3
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F4 as u32,               0x03e), // KEY_F4
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F5 as u32,               0x03f), // KEY_F5
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F6 as u32,               0x040), // KEY_F6
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F7 as u32,               0x041), // KEY_F7
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F8 as u32,               0x042), // KEY_F8
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F9 as u32,               0x043), // KEY_F9
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F10 as u32,              0x044), // KEY_F10
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_NUMLOCKCLEAR as u32,     0x045), // KEY_NUMLOCK
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_SCROLLLOCK as u32,       0x046), // KEY_SCROLLLOCK
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_7 as u32,             0x047), // KEY_KP7
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_8 as u32,             0x048), // KEY_KP8
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_9 as u32,             0x049), // KEY_KP9
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_MINUS as u32,         0x04a), // KEY_KPMINUS
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_4 as u32,             0x04b), // KEY_KP4
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_5 as u32,             0x04c), // KEY_KP5
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_6 as u32,             0x04d), // KEY_KP6
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_PLUS as u32,          0x04e), // KEY_KPPLUS
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_1 as u32,             0x04f), // KEY_KP1
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_2 as u32,             0x050), // KEY_KP2
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_3 as u32,             0x051), // KEY_KP3
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_0 as u32,             0x052), // KEY_KP0
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_PERIOD as u32,        0x053), // KEY_KPDOT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x054),
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LANG5 as u32,            0x055), // KEY_ZENKAKUHANKAKU
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_NONUSBACKSLASH as u32,   0x056), // KEY_102ND
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F11 as u32,              0x057), // KEY_F11
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F12 as u32,              0x058), // KEY_F12
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_INTERNATIONAL1 as u32,   0x059), // KEY_RO
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LANG3 as u32,            0x05a), // KEY_KATAKANA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LANG4 as u32,            0x05b), // KEY_HIRAGANA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_INTERNATIONAL4 as u32,   0x05c), // KEY_HENKAN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_INTERNATIONAL2 as u32,   0x05d), // KEY_KATAKANAHIRAGANA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_INTERNATIONAL5 as u32,   0x05e), // KEY_MUHENKAN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_INTERNATIONAL5 as u32,   0x05f), // KEY_KPJPCOMMA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_ENTER as u32,         0x060), // KEY_KPENTER
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_RCTRL as u32,            0x061), // KEY_RIGHTCTRL
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_DIVIDE as u32,        0x062), // KEY_KPSLASH
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_SYSREQ as u32,           0x063), // KEY_SYSRQ
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_RALT as u32,             0x064), // KEY_RIGHTALT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x065), // KEY_LINEFEED
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_HOME as u32,             0x066), // KEY_HOME
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UP as u32,               0x067), // KEY_UP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_PAGEUP as u32,           0x068), // KEY_PAGEUP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LEFT as u32,             0x069), // KEY_LEFT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_RIGHT as u32,            0x06a), // KEY_RIGHT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_END as u32,              0x06b), // KEY_END
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_DOWN as u32,             0x06c), // KEY_DOWN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_PAGEDOWN as u32,         0x06d), // KEY_PAGEDOWN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_INSERT as u32,           0x06e), // KEY_INSERT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_DELETE as u32,           0x06f), // KEY_DELETE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x070), // KEY_MACRO
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_MUTE as u32,             0x071), // KEY_MUTE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_VOLUMEDOWN as u32,       0x072), // KEY_VOLUMEDOWN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_VOLUMEUP as u32,         0x073), // KEY_VOLUMEUP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_POWER as u32,            0x074), // KEY_POWER
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_EQUALS as u32,        0x075), // KEY_KPEQUAL
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_PLUSMINUS as u32,     0x076), // KEY_KPPLUSMINUS
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_PAUSE as u32,            0x077), // KEY_PAUSE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x078), // KEY_SCALE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_COMMA as u32,         0x079), // KEY_KPCOMMA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LANG1 as u32,            0x07a), // KEY_HANGEUL
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LANG2 as u32,            0x07b), // KEY_HANJA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_INTERNATIONAL3 as u32,   0x07c), // KEY_YEN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_LGUI as u32,             0x07d), // KEY_LEFTMETA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_RGUI as u32,             0x07e), // KEY_RIGHTMETA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_APPLICATION as u32,      0x07f), // KEY_COMPOSE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_STOP as u32,             0x080), // KEY_STOP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AGAIN as u32,            0x081), // KEY_AGAIN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x082), // KEY_PROPS
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNDO as u32,             0x083), // KEY_UNDO
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x084), // KEY_FRONT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_COPY as u32,             0x085), // KEY_COPY
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x086), // KEY_OPEN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_PASTE as u32,            0x087), // KEY_PASTE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_FIND as u32,             0x088), // KEY_FIND
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_CUT as u32,              0x089), // KEY_CUT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_HELP as u32,             0x08a), // KEY_HELP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_MENU as u32,             0x08b), // KEY_MENU
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_CALCULATOR as u32,       0x08c), // KEY_CALC
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x08d), // KEY_SETUP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_SLEEP as u32,            0x08e), // KEY_SLEEP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x08f), // KEY_WAKEUP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x090), // KEY_FILE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x091), // KEY_SENDFILE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x092), // KEY_DELETEFILE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x093), // KEY_XFER
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_APP1 as u32,             0x094), // KEY_PROG1
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_APP2 as u32,             0x095), // KEY_PROG2
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_WWW as u32,              0x096), // KEY_WWW
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x097), // KEY_MSDOS
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x098), // KEY_COFFEE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x099), // KEY_ROTATE_DISPLAY
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x09a), // KEY_CYCLEWINDOWS
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_MAIL as u32,             0x09b), // KEY_MAIL
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AC_BOOKMARKS as u32,     0x09c), // KEY_BOOKMARKS
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_COMPUTER as u32,         0x09d), // KEY_COMPUTER
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AC_BACK as u32,          0x09e), // KEY_BACK
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AC_FORWARD as u32,       0x09f), // KEY_FORWARD
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0a0), // KEY_CLOSECD
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_EJECT as u32,            0x0a1), // KEY_EJECTCD
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_EJECT as u32,            0x0a2), // KEY_EJECTCLOSECD
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AUDIONEXT as u32,        0x0a3), // KEY_NEXTSONG
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AUDIOPLAY as u32,        0x0a4), // KEY_PLAYPAUSE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AUDIOPREV as u32,        0x0a5), // KEY_PREVIOUSSONG
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AUDIOSTOP as u32,        0x0a6), // KEY_STOPCD
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0a7), // KEY_RECORD
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AUDIOREWIND as u32,      0x0a8), // KEY_REWIND
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0a9), // KEY_PHONE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0aa), // KEY_ISO
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0ab), // KEY_CONFIG
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AC_HOME as u32,          0x0ac), // KEY_HOMEPAGE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AC_REFRESH as u32,       0x0ad), // KEY_REFRESH
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0ae), // KEY_EXIT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0af), // KEY_MOVE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0b0), // KEY_EDIT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0b1), // KEY_SCROLLUP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0b2), // KEY_SCROLLDOWN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_LEFTPAREN as u32,     0x0b3), // KEY_KPLEFTPAREN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KP_RIGHTPAREN as u32,    0x0b4), // KEY_KPRIGHTPAREN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0b5), // KEY_NEW
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AGAIN as u32,            0x0b6), // KEY_REDO
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F13 as u32,              0x0b7), // KEY_F13
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F14 as u32,              0x0b8), // KEY_F14
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F15 as u32,              0x0b9), // KEY_F15
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F16 as u32,              0x0ba), // KEY_F16
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F17 as u32,              0x0bb), // KEY_F17
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F18 as u32,              0x0bc), // KEY_F18
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F19 as u32,              0x0bd), // KEY_F19
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F20 as u32,              0x0be), // KEY_F20
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F21 as u32,              0x0bf), // KEY_F21
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F22 as u32,              0x0c0), // KEY_F22
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F23 as u32,              0x0c1), // KEY_F23
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_F24 as u32,              0x0c2), // KEY_F24
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0c3),
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0c4),
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0c5),
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0c6),
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0c7),
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AUDIOPLAY as u32,        0x0c8), // KEY_PLAYCD
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0c9), // KEY_PAUSECD
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0ca), // KEY_PROG3
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0cb), // KEY_PROG4
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0cc), // KEY_ALL_APPLICATIONS
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0cd), // KEY_SUSPEND
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0ce), // KEY_CLOSE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AUDIOPLAY as u32,        0x0cf), // KEY_PLAY
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AUDIOFASTFORWARD as u32, 0x0d0), // KEY_FASTFORWARD
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0d1), // KEY_BASSBOOST
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_PRINTSCREEN as u32,      0x0d2), // KEY_PRINT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0d3), // KEY_HP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0d4), // KEY_CAMERA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0d5), // KEY_SOUND
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0d6), // KEY_QUESTION
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_MAIL as u32,             0x0d7), // KEY_EMAIL
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0d8), // KEY_CHAT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_AC_SEARCH as u32,        0x0d9), // KEY_SEARCH
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0da), // KEY_CONNECT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0db), // KEY_FINANCE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0dc), // KEY_SPORT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0dd), // KEY_SHOP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_ALTERASE as u32,         0x0de), // KEY_ALTERASE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_CANCEL as u32,           0x0df), // KEY_CANCEL
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_BRIGHTNESSDOWN as u32,   0x0e0), // KEY_BRIGHTNESSDOWN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_BRIGHTNESSUP as u32,     0x0e1), // KEY_BRIGHTNESSUP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_MEDIASELECT as u32,      0x0e2), // KEY_MEDIA
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_DISPLAYSWITCH as u32,    0x0e3), // KEY_SWITCHVIDEOMODE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KBDILLUMTOGGLE as u32,   0x0e4), // KEY_KBDILLUMTOGGLE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KBDILLUMDOWN as u32,     0x0e5), // KEY_KBDILLUMDOWN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_KBDILLUMUP as u32,       0x0e6), // KEY_KBDILLUMUP
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0e7), // KEY_SEND
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0e8), // KEY_REPLY
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0e9), // KEY_FORWARDMAIL
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0ea), // KEY_SAVE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0eb), // KEY_DOCUMENTS
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0ec), // KEY_BATTERY
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0ed), // KEY_BLUETOOTH
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0ee), // KEY_WLAN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0ef), // KEY_UWB
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0f0), // KEY_UNKNOWN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0f1), // KEY_VIDEO_NEXT
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0f2), // KEY_VIDEO_PREV
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0f3), // KEY_BRIGHTNESS_CYCLE
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0f4), // KEY_BRIGHTNESS_AUTO
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0f5), // KEY_DISPLAY_OFF
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0f6), // KEY_WWAN
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0f7), // KEY_RFKILL
                (sdl2_sys::SDL_Scancode::SDL_SCANCODE_UNKNOWN as u32,          0x0f8), // KEY_MICMUTE
                ],
        };
}

/// Keycodes for each possible key in user input
///
/// These codes identify the keys represented by codes in the
/// events to allow for easy matching on the user side.
///
/// These are numerically the same as the constants in SDL2.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum Keycode {
    UNKNOWN,
    RETURN,
    ESCAPE,
    BACKSPACE,
    TAB,
    SPACE,
    EXCLAIM,
    QUOTEDBL,
    HASH,
    PERCENT,
    DOLLAR,
    AMPERSAND,
    QUOTE,
    LEFTPAREN,
    RIGHTPAREN,
    ASTERISK,
    PLUS,
    COMMA,
    MINUS,
    PERIOD,
    SLASH,
    NUM0,
    NUM1,
    NUM2,
    NUM3,
    NUM4,
    NUM5,
    NUM6,
    NUM7,
    NUM8,
    NUM9,
    COLON,
    SEMICOLON,
    LESS,
    EQUALS,
    GREATER,
    QUESTION,
    AT,
    LEFTBRACKET,
    BACKSLASH,
    RIGHTBRACKET,
    CARET,
    UNDERSCORE,
    BACKQUOTE,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    CAPSLOCK,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    PRINTSCREEN,
    SCROLLLOCK,
    PAUSE,
    INSERT,
    HOME,
    PAGEUP,
    DELETE,
    END,
    PAGEDOWN,
    RIGHT,
    LEFT,
    DOWN,
    UP,
    NUMLOCK,
    KP_DIVIDE,
    KP_MULTIPLY,
    KP_MINUS,
    KP_PLUS,
    KP_ENTER,
    KP_1,
    KP_2,
    KP_3,
    KP_4,
    KP_5,
    KP_6,
    KP_7,
    KP_8,
    KP_9,
    KP_0,
    KP_PERIOD,
    APPLICATION,
    POWER,
    KP_EQUALS,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    EXECUTE,
    HELP,
    MENU,
    SELECT,
    STOP,
    REDO,
    UNDO,
    CUT,
    COPY,
    PASTE,
    FIND,
    MUTE,
    VOLUMEUP,
    VOLUMEDOWN,
    KP_COMMA,
    KP_EQUALSAS400,
    ALTERASE,
    SYSREQ,
    CANCEL,
    CLEAR,
    PRIOR,
    RETURN2,
    SEPARATOR,
    OUT,
    OPER,
    CLEARAGAIN,
    CRSEL,
    EXSEL,
    KP_00,
    KP_000,
    THOUSANDSSEPARATOR,
    DECIMALSEPARATOR,
    CURRENCYUNIT,
    CURRENCYSUBUNIT,
    KP_LEFTPAREN,
    KP_RIGHTPAREN,
    KP_LEFTBRACE,
    KP_RIGHTBRACE,
    KP_TAB,
    KP_BACKSPACE,
    KP_A,
    KP_B,
    KP_C,
    KP_D,
    KP_E,
    KP_F,
    KP_XOR,
    KP_POWER,
    KP_PERCENT,
    KP_LESS,
    KP_GREATER,
    KP_AMPERSAND,
    KP_DBLAMPERSAND,
    VERTICALBAR,
    KP_VERTICALBAR,
    KP_DBLVERTICALBAR,
    KP_COLON,
    KP_HASH,
    KP_SPACE,
    KP_AT,
    KP_EXCLAM,
    KP_MEMSTORE,
    KP_MEMRECALL,
    KP_MEMCLEAR,
    KP_MEMADD,
    KP_MEMSUBTRACT,
    KP_MEMMULTIPLY,
    KP_MEMDIVIDE,
    KP_PLUSMINUS,
    KP_CLEAR,
    KP_CLEARENTRY,
    KP_BINARY,
    KP_OCTAL,
    KP_DECIMAL,
    KP_HEXADECIMAL,
    LCTRL,
    LSHIFT,
    LALT,
    LGUI,
    RCTRL,
    RSHIFT,
    RALT,
    RGUI,
    MODE,
    AUDIONEXT,
    AUDIOPREV,
    AUDIOSTOP,
    AUDIOPLAY,
    AUDIOMUTE,
    MEDIASELECT,
    WWW,
    MAIL,
    CALCULATOR,
    COMPUTER,
    AC_SEARCH,
    AC_HOME,
    AC_BACK,
    AC_FORWARD,
    AC_STOP,
    AC_REFRESH,
    AC_BOOKMARKS,
    BRIGHTNESSDOWN,
    BRIGHTNESSUP,
    DISPLAYSWITCH,
    KBDILLUMTOGGLE,
    KBDILLUMDOWN,
    KBDILLUMUP,
    EJECT,
    SLEEP,
}

impl Keycode {
    /// Returns true if this Keycode is a modifier key
    ///
    /// Modifiers are treated separately than regular keypresses. Keycodes
    /// are still used for tracking them but the user will use the Mods event.
    pub fn is_modifier(&self) -> bool {
        match self {
            Self::LCTRL
            | Self::LSHIFT
            | Self::LALT
            | Self::LGUI
            | Self::RCTRL
            | Self::RSHIFT
            | Self::RALT
            | Self::RGUI => true,
            _ => false,
        }
    }
}

/// Convert an xkbcommon keycode into a Dakota Keycode
///
/// This handles looking up the keycode translation using an internal lookup table.
///
/// TODO: Make this O(1)
#[cfg(feature = "direct2display")]
pub fn convert_xkb_keycode_to_dakota(key: u32) -> Keycode {
    CT_XKB_TO_DAKOTA.val_to_key(key).unwrap_or(Keycode::UNKNOWN)
}

/// Convert an SDL keycode into a Dakota Keycode
///
/// This handles looking up the keycode translation using an internal lookup table.
///
/// TODO: Make this O(1)
#[cfg(feature = "sdl")]
pub fn convert_sdl_keycode_to_dakota(key: sdl2::keyboard::Keycode) -> Keycode {
    CT_SDL_TO_DAKOTA.val_to_key(key).unwrap_or(Keycode::UNKNOWN)
}

/// Convert an SDL scancode into a Linux `KEY_*` value
///
/// One of the issues is that SDL does not support giving us a unicode value for a keypress,
/// so instead we have to feed it through xkbcommon. This means we need to look up a linux
/// key encoding value for the key pressed with SDL, which is done here.
///
/// TODO: Make this O(1)
#[cfg(feature = "sdl")]
pub fn convert_sdl_scancode_to_linux(code: sdl2::keyboard::Scancode) -> u32 {
    CT_SDL_TO_LINUX_KEY.key_to_val(code as u32).unwrap_or(0) // Unknown
}
