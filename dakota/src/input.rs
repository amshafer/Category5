// Constants for input operations
//
// These are taken from SDL2
//
// Austin Shafer - 2020

extern crate num;
extern crate num_derive;
use num_derive::FromPrimitive;

bitflags::bitflags! {
    pub struct Mods: u16 {
        const NOMOD = 0x0000;
        const LSHIFTMOD = 0x0001;
        const RSHIFTMOD = 0x0002;
        const LCTRLMOD = 0x0040;
        const RCTRLMOD = 0x0080;
        const LALTMOD = 0x0100;
        const RALTMOD = 0x0200;
        const LGUIMOD = 0x0400;
        const RGUIMOD = 0x0800;
        const NUMMOD = 0x1000;
        const CAPSMOD = 0x2000;
        const MODEMOD = 0x4000;
        const RESERVEDMOD = 0x8000;
    }
}

#[cfg(feature = "sdl")]
pub fn convert_sdl_mods_to_dakota(keymods: sdl2::keyboard::Mod) -> Mods {
    Mods::from_bits(keymods.bits()).expect("Invalid mod bits")
}

/// Keycodes for mouse buttons.
///
/// Names are self explanitory, `LEFT` for left click and etc.
///
/// These also match SDL2 and libinput's keycodes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum MouseButton {
    UNKNOWN = 0,
    LEFT = 1,
    MIDDLE = 2,
    RIGHT = 3,
    X1 = 4,
    X2 = 5,
}

#[cfg(feature = "sdl")]
pub fn convert_sdl_mouse_to_dakota(button: sdl2::mouse::MouseButton) -> MouseButton {
    match button {
        sdl2::mouse::MouseButton::Left => MouseButton::LEFT,
        sdl2::mouse::MouseButton::Middle => MouseButton::MIDDLE,
        sdl2::mouse::MouseButton::Right => MouseButton::RIGHT,
        sdl2::mouse::MouseButton::X1 => MouseButton::X1,
        sdl2::mouse::MouseButton::X2 => MouseButton::X2,
        _ => MouseButton::UNKNOWN,
    }
}

/// Keycodes for each possible key in user input
///
/// These codes identify the keys represented by codes in the
/// events to allow for easy matching on the user side.
///
/// These are numerically the same as the constants in SDL2.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive)]
#[allow(non_camel_case_types)]
pub enum Keycode {
    UNKNOWN = 0,
    RETURN = 13,
    ESCAPE = 27,
    BACKSPACE = 8,
    TAB = 9,
    SPACE = 32,
    EXCLAIM = 33,
    QUOTEDBL = 34,
    HASH = 35,
    PERCENT = 37,
    DOLLAR = 36,
    AMPERSAND = 38,
    QUOTE = 39,
    LEFTPAREN = 40,
    RIGHTPAREN = 41,
    ASTERISK = 42,
    PLUS = 43,
    COMMA = 44,
    MINUS = 45,
    PERIOD = 46,
    SLASH = 47,
    NUM0 = 48,
    NUM1 = 49,
    NUM2 = 50,
    NUM3 = 51,
    NUM4 = 52,
    NUM5 = 53,
    NUM6 = 54,
    NUM7 = 55,
    NUM8 = 56,
    NUM9 = 57,
    COLON = 58,
    SEMICOLON = 59,
    LESS = 60,
    EQUALS = 61,
    GREATER = 62,
    QUESTION = 63,
    AT = 64,
    LEFTBRACKET = 91,
    BACKSLASH = 92,
    RIGHTBRACKET = 93,
    CARET = 94,
    UNDERSCORE = 95,
    BACKQUOTE = 96,
    A = 97,
    B = 98,
    C = 99,
    D = 100,
    E = 101,
    F = 102,
    G = 103,
    H = 104,
    I = 105,
    J = 106,
    K = 107,
    L = 108,
    M = 109,
    N = 110,
    O = 111,
    P = 112,
    Q = 113,
    R = 114,
    S = 115,
    T = 116,
    U = 117,
    V = 118,
    W = 119,
    X = 120,
    Y = 121,
    Z = 122,
    CAPSLOCK = 1073741881,
    F1 = 1073741882,
    F2 = 1073741883,
    F3 = 1073741884,
    F4 = 1073741885,
    F5 = 1073741886,
    F6 = 1073741887,
    F7 = 1073741888,
    F8 = 1073741889,
    F9 = 1073741890,
    F10 = 1073741891,
    F11 = 1073741892,
    F12 = 1073741893,
    PRINTSCREEN = 1073741894,
    SCROLLLOCK = 1073741895,
    PAUSE = 1073741896,
    INSERT = 1073741897,
    HOME = 1073741898,
    PAGEUP = 1073741899,
    DELETE = 127,
    END = 1073741901,
    PAGEDOWN = 1073741902,
    RIGHT = 1073741903,
    LEFT = 1073741904,
    DOWN = 1073741905,
    UP = 1073741906,
    NUMLOCKCLEAR = 1073741907,
    KP_DIVIDE = 1073741908,
    KP_MULTIPLY = 1073741909,
    KP_MINUS = 1073741910,
    KP_PLUS = 1073741911,
    KP_ENTER = 1073741912,
    KP_1 = 1073741913,
    KP_2 = 1073741914,
    KP_3 = 1073741915,
    KP_4 = 1073741916,
    KP_5 = 1073741917,
    KP_6 = 1073741918,
    KP_7 = 1073741919,
    KP_8 = 1073741920,
    KP_9 = 1073741921,
    KP_0 = 1073741922,
    KP_PERIOD = 1073741923,
    APPLICATION = 1073741925,
    POWER = 1073741926,
    KP_EQUALS = 1073741927,
    F13 = 1073741928,
    F14 = 1073741929,
    F15 = 1073741930,
    F16 = 1073741931,
    F17 = 1073741932,
    F18 = 1073741933,
    F19 = 1073741934,
    F20 = 1073741935,
    F21 = 1073741936,
    F22 = 1073741937,
    F23 = 1073741938,
    F24 = 1073741939,
    EXECUTE = 1073741940,
    HELP = 1073741941,
    MENU = 1073741942,
    SELECT = 1073741943,
    STOP = 1073741944,
    AGAIN = 1073741945,
    UNDO = 1073741946,
    CUT = 1073741947,
    COPY = 1073741948,
    PASTE = 1073741949,
    FIND = 1073741950,
    MUTE = 1073741951,
    VOLUMEUP = 1073741952,
    VOLUMEDOWN = 1073741953,
    KP_COMMA = 1073741957,
    KP_EQUALSAS400 = 1073741958,
    ALTERASE = 1073741977,
    SYSREQ = 1073741978,
    CANCEL = 1073741979,
    CLEAR = 1073741980,
    PRIOR = 1073741981,
    RETURN2 = 1073741982,
    SEPARATOR = 1073741983,
    OUT = 1073741984,
    OPER = 1073741985,
    CLEARAGAIN = 1073741986,
    CRSEL = 1073741987,
    EXSEL = 1073741988,
    KP_00 = 1073742000,
    KP_000 = 1073742001,
    THOUSANDSSEPARATOR = 1073742002,
    DECIMALSEPARATOR = 1073742003,
    CURRENCYUNIT = 1073742004,
    CURRENCYSUBUNIT = 1073742005,
    KP_LEFTPAREN = 1073742006,
    KP_RIGHTPAREN = 1073742007,
    KP_LEFTBRACE = 1073742008,
    KP_RIGHTBRACE = 1073742009,
    KP_TAB = 1073742010,
    KP_BACKSPACE = 1073742011,
    KP_A = 1073742012,
    KP_B = 1073742013,
    KP_C = 1073742014,
    KP_D = 1073742015,
    KP_E = 1073742016,
    KP_F = 1073742017,
    KP_XOR = 1073742018,
    KP_POWER = 1073742019,
    KP_PERCENT = 1073742020,
    KP_LESS = 1073742021,
    KP_GREATER = 1073742022,
    KP_AMPERSAND = 1073742023,
    KP_DBLAMPERSAND = 1073742024,
    KP_VERTICALBAR = 1073742025,
    KP_DBLVERTICALBAR = 1073742026,
    KP_COLON = 1073742027,
    KP_HASH = 1073742028,
    KP_SPACE = 1073742029,
    KP_AT = 1073742030,
    KP_EXCLAM = 1073742031,
    KP_MEMSTORE = 1073742032,
    KP_MEMRECALL = 1073742033,
    KP_MEMCLEAR = 1073742034,
    KP_MEMADD = 1073742035,
    KP_MEMSUBTRACT = 1073742036,
    KP_MEMMULTIPLY = 1073742037,
    KP_MEMDIVIDE = 1073742038,
    KP_PLUSMINUS = 1073742039,
    KP_CLEAR = 1073742040,
    KP_CLEARENTRY = 1073742041,
    KP_BINARY = 1073742042,
    KP_OCTAL = 1073742043,
    KP_DECIMAL = 1073742044,
    KP_HEXADECIMAL = 1073742045,
    LCTRL = 1073742048,
    LSHIFT = 1073742049,
    LALT = 1073742050,
    LGUI = 1073742051,
    RCTRL = 1073742052,
    RSHIFT = 1073742053,
    RALT = 1073742054,
    RGUI = 1073742055,
    MODE = 1073742081,
    AUDIONEXT = 1073742082,
    AUDIOPREV = 1073742083,
    AUDIOSTOP = 1073742084,
    AUDIOPLAY = 1073742085,
    AUDIOMUTE = 1073742086,
    MEDIASELECT = 1073742087,
    WWW = 1073742088,
    MAIL = 1073742089,
    CALCULATOR = 1073742090,
    COMPUTER = 1073742091,
    AC_SEARCH = 1073742092,
    AC_HOME = 1073742093,
    AC_BACK = 1073742094,
    AC_FORWARD = 1073742095,
    AC_STOP = 1073742096,
    AC_REFRESH = 1073742097,
    AC_BOOKMARKS = 1073742098,
    BRIGHTNESSDOWN = 1073742099,
    BRIGHTNESSUP = 1073742100,
    DISPLAYSWITCH = 1073742101,
    KBDILLUMTOGGLE = 1073742102,
    KBDILLUMDOWN = 1073742103,
    KBDILLUMUP = 1073742104,
    EJECT = 1073742105,
    SLEEP = 1073742106,
    APP1 = 1073742107,
    APP2 = 1073742108,
    AUDIOREWIND = 1073742109,
    AUDIOFASTFORWARD = 1073742110,
}

// This is a nasty implementation, but it's unfortunately the
// easiest one. This is what a procedural macro would do, but it avoids
// some of the nasty macro writing by just doing it in vim.
#[cfg(feature = "sdl")]
pub fn convert_sdl_keycode_to_dakota(key: sdl2::keyboard::Keycode) -> Keycode {
    num::FromPrimitive::from_u32(key as u32).expect("Invalid Keycode, not recognized")
}
