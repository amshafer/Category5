use std::os::raw::{c_char, c_void};
const NULLPTR: *const c_void = 0 as *const c_void;
static mut types_null: [*const sys::common::wl_interface; 1] =
    [NULLPTR as *const sys::common::wl_interface];
pub mod wl_drm {
    use super::sys::common::{wl_argument, wl_array, wl_interface, wl_message};
    use super::sys::server::*;
    use super::{
        smallvec, types_null, AnonymousObject, Argument, ArgumentType, Interface, Main, Message,
        MessageDesc, MessageGroup, Object, ObjectMetadata, Resource, NULLPTR,
    };
    use std::os::raw::c_char;
    #[repr(u32)]
    #[derive(Copy, Clone, Debug, PartialEq)]
    #[non_exhaustive]
    pub enum Error {
        AuthenticateFail = 0,
        InvalidFormat = 1,
        InvalidName = 2,
    }
    impl Error {
        pub fn from_raw(n: u32) -> Option<Error> {
            match n {
                0 => Some(Error::AuthenticateFail),
                1 => Some(Error::InvalidFormat),
                2 => Some(Error::InvalidName),
                _ => Option::None,
            }
        }
        pub fn to_raw(&self) -> u32 {
            *self as u32
        }
    }
    #[repr(u32)]
    #[derive(Copy, Clone, Debug, PartialEq)]
    #[non_exhaustive]
    pub enum Format {
        C8 = 538982467,
        Rgb332 = 943867730,
        Bgr233 = 944916290,
        Xrgb4444 = 842093144,
        Xbgr4444 = 842089048,
        Rgbx4444 = 842094674,
        Bgrx4444 = 842094658,
        Argb4444 = 842093121,
        Abgr4444 = 842089025,
        Rgba4444 = 842088786,
        Bgra4444 = 842088770,
        Xrgb1555 = 892424792,
        Xbgr1555 = 892420696,
        Rgbx5551 = 892426322,
        Bgrx5551 = 892426306,
        Argb1555 = 892424769,
        Abgr1555 = 892420673,
        Rgba5551 = 892420434,
        Bgra5551 = 892420418,
        Rgb565 = 909199186,
        Bgr565 = 909199170,
        Rgb888 = 875710290,
        Bgr888 = 875710274,
        Xrgb8888 = 875713112,
        Xbgr8888 = 875709016,
        Rgbx8888 = 875714642,
        Bgrx8888 = 875714626,
        Argb8888 = 875713089,
        Abgr8888 = 875708993,
        Rgba8888 = 875708754,
        Bgra8888 = 875708738,
        Xrgb2101010 = 808669784,
        Xbgr2101010 = 808665688,
        Rgbx1010102 = 808671314,
        Bgrx1010102 = 808671298,
        Argb2101010 = 808669761,
        Abgr2101010 = 808665665,
        Rgba1010102 = 808665426,
        Bgra1010102 = 808665410,
        Yuyv = 1448695129,
        Yvyu = 1431918169,
        Uyvy = 1498831189,
        Vyuy = 1498765654,
        Ayuv = 1448433985,
        Xyuv8888 = 1448434008,
        Nv12 = 842094158,
        Nv21 = 825382478,
        Nv16 = 909203022,
        Nv61 = 825644622,
        Yuv410 = 961959257,
        Yvu410 = 961893977,
        Yuv411 = 825316697,
        Yvu411 = 825316953,
        Yuv420 = 842093913,
        Yvu420 = 842094169,
        Yuv422 = 909202777,
        Yvu422 = 909203033,
        Yuv444 = 875713881,
        Yvu444 = 875714137,
        Abgr16f = 1211384385,
        Xbgr16f = 1211384408,
    }
    impl Format {
        pub fn from_raw(n: u32) -> Option<Format> {
            match n {
                538982467 => Some(Format::C8),
                943867730 => Some(Format::Rgb332),
                944916290 => Some(Format::Bgr233),
                842093144 => Some(Format::Xrgb4444),
                842089048 => Some(Format::Xbgr4444),
                842094674 => Some(Format::Rgbx4444),
                842094658 => Some(Format::Bgrx4444),
                842093121 => Some(Format::Argb4444),
                842089025 => Some(Format::Abgr4444),
                842088786 => Some(Format::Rgba4444),
                842088770 => Some(Format::Bgra4444),
                892424792 => Some(Format::Xrgb1555),
                892420696 => Some(Format::Xbgr1555),
                892426322 => Some(Format::Rgbx5551),
                892426306 => Some(Format::Bgrx5551),
                892424769 => Some(Format::Argb1555),
                892420673 => Some(Format::Abgr1555),
                892420434 => Some(Format::Rgba5551),
                892420418 => Some(Format::Bgra5551),
                909199186 => Some(Format::Rgb565),
                909199170 => Some(Format::Bgr565),
                875710290 => Some(Format::Rgb888),
                875710274 => Some(Format::Bgr888),
                875713112 => Some(Format::Xrgb8888),
                875709016 => Some(Format::Xbgr8888),
                875714642 => Some(Format::Rgbx8888),
                875714626 => Some(Format::Bgrx8888),
                875713089 => Some(Format::Argb8888),
                875708993 => Some(Format::Abgr8888),
                875708754 => Some(Format::Rgba8888),
                875708738 => Some(Format::Bgra8888),
                808669784 => Some(Format::Xrgb2101010),
                808665688 => Some(Format::Xbgr2101010),
                808671314 => Some(Format::Rgbx1010102),
                808671298 => Some(Format::Bgrx1010102),
                808669761 => Some(Format::Argb2101010),
                808665665 => Some(Format::Abgr2101010),
                808665426 => Some(Format::Rgba1010102),
                808665410 => Some(Format::Bgra1010102),
                1448695129 => Some(Format::Yuyv),
                1431918169 => Some(Format::Yvyu),
                1498831189 => Some(Format::Uyvy),
                1498765654 => Some(Format::Vyuy),
                1448433985 => Some(Format::Ayuv),
                1448434008 => Some(Format::Xyuv8888),
                842094158 => Some(Format::Nv12),
                825382478 => Some(Format::Nv21),
                909203022 => Some(Format::Nv16),
                825644622 => Some(Format::Nv61),
                961959257 => Some(Format::Yuv410),
                961893977 => Some(Format::Yvu410),
                825316697 => Some(Format::Yuv411),
                825316953 => Some(Format::Yvu411),
                842093913 => Some(Format::Yuv420),
                842094169 => Some(Format::Yvu420),
                909202777 => Some(Format::Yuv422),
                909203033 => Some(Format::Yvu422),
                875713881 => Some(Format::Yuv444),
                875714137 => Some(Format::Yvu444),
                1211384385 => Some(Format::Abgr16f),
                1211384408 => Some(Format::Xbgr16f),
                _ => Option::None,
            }
        }
        pub fn to_raw(&self) -> u32 {
            *self as u32
        }
    }
    #[doc = "wl_drm capability bitmask\n\nBitmask of capabilities."]
    #[repr(u32)]
    #[derive(Copy, Clone, Debug, PartialEq)]
    #[non_exhaustive]
    pub enum Capability {
        #[doc = "wl_drm prime available"]
        Prime = 1,
    }
    impl Capability {
        pub fn from_raw(n: u32) -> Option<Capability> {
            match n {
                1 => Some(Capability::Prime),
                _ => Option::None,
            }
        }
        pub fn to_raw(&self) -> u32 {
            *self as u32
        }
    }
    #[derive(Debug)]
    #[non_exhaustive]
    pub enum Request {
        #[doc = ""]
        Authenticate { id: u32 },
        #[doc = ""]
        CreateBuffer {
            id: Main<super::wl_buffer::WlBuffer>,
            name: u32,
            width: i32,
            height: i32,
            stride: u32,
            format: u32,
        },
        #[doc = ""]
        CreatePlanarBuffer {
            id: Main<super::wl_buffer::WlBuffer>,
            name: u32,
            width: i32,
            height: i32,
            format: u32,
            offset0: i32,
            stride0: i32,
            offset1: i32,
            stride1: i32,
            offset2: i32,
            stride2: i32,
        },
        #[doc = "Only available since version 2 of the interface"]
        CreatePrimeBuffer {
            id: Main<super::wl_buffer::WlBuffer>,
            name: ::std::os::unix::io::RawFd,
            width: i32,
            height: i32,
            format: u32,
            offset0: i32,
            stride0: i32,
            offset1: i32,
            stride1: i32,
            offset2: i32,
            stride2: i32,
        },
    }
    impl super::MessageGroup for Request {
        const MESSAGES: &'static [super::MessageDesc] = &[
            super::MessageDesc {
                name: "authenticate",
                since: 1,
                signature: &[super::ArgumentType::Uint],
                destructor: false,
            },
            super::MessageDesc {
                name: "create_buffer",
                since: 1,
                signature: &[
                    super::ArgumentType::NewId,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Uint,
                ],
                destructor: false,
            },
            super::MessageDesc {
                name: "create_planar_buffer",
                since: 1,
                signature: &[
                    super::ArgumentType::NewId,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                ],
                destructor: false,
            },
            super::MessageDesc {
                name: "create_prime_buffer",
                since: 2,
                signature: &[
                    super::ArgumentType::NewId,
                    super::ArgumentType::Fd,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                ],
                destructor: false,
            },
        ];
        type Map = super::ResourceMap;
        fn is_destructor(&self) -> bool {
            match *self {
                _ => false,
            }
        }
        fn opcode(&self) -> u16 {
            match *self {
                Request::Authenticate { .. } => 0,
                Request::CreateBuffer { .. } => 1,
                Request::CreatePlanarBuffer { .. } => 2,
                Request::CreatePrimeBuffer { .. } => 3,
            }
        }
        fn since(&self) -> u32 {
            match *self {
                Request::Authenticate { .. } => 1,
                Request::CreateBuffer { .. } => 1,
                Request::CreatePlanarBuffer { .. } => 1,
                Request::CreatePrimeBuffer { .. } => 2,
            }
        }
        fn child<Meta: ObjectMetadata>(
            opcode: u16,
            version: u32,
            meta: &Meta,
        ) -> Option<Object<Meta>> {
            match opcode {
                1 => Some(Object::from_interface::<super::wl_buffer::WlBuffer>(
                    version,
                    meta.child(),
                )),
                2 => Some(Object::from_interface::<super::wl_buffer::WlBuffer>(
                    version,
                    meta.child(),
                )),
                3 => Some(Object::from_interface::<super::wl_buffer::WlBuffer>(
                    version,
                    meta.child(),
                )),
                _ => None,
            }
        }
        fn from_raw(msg: Message, map: &mut Self::Map) -> Result<Self, ()> {
            match msg.opcode {
                0 => {
                    let mut args = msg.args.into_iter();
                    Ok(Request::Authenticate {
                        id: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                    })
                }
                1 => {
                    let mut args = msg.args.into_iter();
                    Ok(Request::CreateBuffer {
                        id: {
                            if let Some(Argument::NewId(val)) = args.next() {
                                map.get_new(val).ok_or(())?
                            } else {
                                return Err(());
                            }
                        },
                        name: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        width: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        height: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        stride: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        format: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                    })
                }
                2 => {
                    let mut args = msg.args.into_iter();
                    Ok(Request::CreatePlanarBuffer {
                        id: {
                            if let Some(Argument::NewId(val)) = args.next() {
                                map.get_new(val).ok_or(())?
                            } else {
                                return Err(());
                            }
                        },
                        name: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        width: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        height: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        format: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        offset0: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        stride0: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        offset1: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        stride1: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        offset2: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        stride2: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                    })
                }
                3 => {
                    let mut args = msg.args.into_iter();
                    Ok(Request::CreatePrimeBuffer {
                        id: {
                            if let Some(Argument::NewId(val)) = args.next() {
                                map.get_new(val).ok_or(())?
                            } else {
                                return Err(());
                            }
                        },
                        name: {
                            if let Some(Argument::Fd(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        width: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        height: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        format: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        offset0: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        stride0: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        offset1: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        stride1: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        offset2: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        stride2: {
                            if let Some(Argument::Int(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                    })
                }
                _ => Err(()),
            }
        }
        fn into_raw(self, sender_id: u32) -> Message {
            panic!("Request::into_raw can not be used Server-side.")
        }
        unsafe fn from_raw_c(
            obj: *mut ::std::os::raw::c_void,
            opcode: u32,
            args: *const wl_argument,
        ) -> Result<Request, ()> {
            match opcode {
                0 => {
                    let _args = ::std::slice::from_raw_parts(args, 1);
                    Ok(Request::Authenticate { id: _args[0].u })
                }
                1 => {
                    let _args = ::std::slice::from_raw_parts(args, 6);
                    Ok(Request::CreateBuffer {
                        id: {
                            let me = Resource::<WlDrm>::from_c_ptr(obj as *mut _);
                            me.make_child_for::<super::wl_buffer::WlBuffer>(_args[0].n)
                                .unwrap()
                        },
                        name: _args[1].u,
                        width: _args[2].i,
                        height: _args[3].i,
                        stride: _args[4].u,
                        format: _args[5].u,
                    })
                }
                2 => {
                    let _args = ::std::slice::from_raw_parts(args, 11);
                    Ok(Request::CreatePlanarBuffer {
                        id: {
                            let me = Resource::<WlDrm>::from_c_ptr(obj as *mut _);
                            me.make_child_for::<super::wl_buffer::WlBuffer>(_args[0].n)
                                .unwrap()
                        },
                        name: _args[1].u,
                        width: _args[2].i,
                        height: _args[3].i,
                        format: _args[4].u,
                        offset0: _args[5].i,
                        stride0: _args[6].i,
                        offset1: _args[7].i,
                        stride1: _args[8].i,
                        offset2: _args[9].i,
                        stride2: _args[10].i,
                    })
                }
                3 => {
                    let _args = ::std::slice::from_raw_parts(args, 11);
                    Ok(Request::CreatePrimeBuffer {
                        id: {
                            let me = Resource::<WlDrm>::from_c_ptr(obj as *mut _);
                            me.make_child_for::<super::wl_buffer::WlBuffer>(_args[0].n)
                                .unwrap()
                        },
                        name: _args[1].h,
                        width: _args[2].i,
                        height: _args[3].i,
                        format: _args[4].u,
                        offset0: _args[5].i,
                        stride0: _args[6].i,
                        offset1: _args[7].i,
                        stride1: _args[8].i,
                        offset2: _args[9].i,
                        stride2: _args[10].i,
                    })
                }
                _ => return Err(()),
            }
        }
        fn as_raw_c_in<F, T>(self, f: F) -> T
        where
            F: FnOnce(u32, &mut [wl_argument]) -> T,
        {
            panic!("Request::as_raw_c_in can not be used Server-side.")
        }
    }
    #[derive(Debug)]
    #[non_exhaustive]
    pub enum Event {
        #[doc = ""]
        Device { name: String },
        #[doc = ""]
        Format { format: u32 },
        #[doc = ""]
        Authenticated,
        #[doc = ""]
        Capabilities { value: u32 },
    }
    impl super::MessageGroup for Event {
        const MESSAGES: &'static [super::MessageDesc] = &[
            super::MessageDesc {
                name: "device",
                since: 1,
                signature: &[super::ArgumentType::Str],
                destructor: false,
            },
            super::MessageDesc {
                name: "format",
                since: 1,
                signature: &[super::ArgumentType::Uint],
                destructor: false,
            },
            super::MessageDesc {
                name: "authenticated",
                since: 1,
                signature: &[],
                destructor: false,
            },
            super::MessageDesc {
                name: "capabilities",
                since: 1,
                signature: &[super::ArgumentType::Uint],
                destructor: false,
            },
        ];
        type Map = super::ResourceMap;
        fn is_destructor(&self) -> bool {
            match *self {
                _ => false,
            }
        }
        fn opcode(&self) -> u16 {
            match *self {
                Event::Device { .. } => 0,
                Event::Format { .. } => 1,
                Event::Authenticated => 2,
                Event::Capabilities { .. } => 3,
            }
        }
        fn since(&self) -> u32 {
            match *self {
                Event::Device { .. } => 1,
                Event::Format { .. } => 1,
                Event::Authenticated => 1,
                Event::Capabilities { .. } => 1,
            }
        }
        fn child<Meta: ObjectMetadata>(
            opcode: u16,
            version: u32,
            meta: &Meta,
        ) -> Option<Object<Meta>> {
            match opcode {
                _ => None,
            }
        }
        fn from_raw(msg: Message, map: &mut Self::Map) -> Result<Self, ()> {
            panic!("Event::from_raw can not be used Server-side.")
        }
        fn into_raw(self, sender_id: u32) -> Message {
            match self {
                Event::Device { name } => Message {
                    sender_id: sender_id,
                    opcode: 0,
                    args: smallvec![Argument::Str(Box::new(unsafe {
                        ::std::ffi::CString::from_vec_unchecked(name.into())
                    })),],
                },
                Event::Format { format } => Message {
                    sender_id: sender_id,
                    opcode: 1,
                    args: smallvec![Argument::Uint(format),],
                },
                Event::Authenticated => Message {
                    sender_id: sender_id,
                    opcode: 2,
                    args: smallvec![],
                },
                Event::Capabilities { value } => Message {
                    sender_id: sender_id,
                    opcode: 3,
                    args: smallvec![Argument::Uint(value),],
                },
            }
        }
        unsafe fn from_raw_c(
            obj: *mut ::std::os::raw::c_void,
            opcode: u32,
            args: *const wl_argument,
        ) -> Result<Event, ()> {
            panic!("Event::from_raw_c can not be used Server-side.")
        }
        fn as_raw_c_in<F, T>(self, f: F) -> T
        where
            F: FnOnce(u32, &mut [wl_argument]) -> T,
        {
            match self {
                Event::Device { name } => {
                    let mut _args_array: [wl_argument; 1] = unsafe { ::std::mem::zeroed() };
                    let _arg_0 = ::std::ffi::CString::new(name).unwrap();
                    _args_array[0].s = _arg_0.as_ptr();
                    f(0, &mut _args_array)
                }
                Event::Format { format } => {
                    let mut _args_array: [wl_argument; 1] = unsafe { ::std::mem::zeroed() };
                    _args_array[0].u = format;
                    f(1, &mut _args_array)
                }
                Event::Authenticated => {
                    let mut _args_array: [wl_argument; 0] = unsafe { ::std::mem::zeroed() };
                    f(2, &mut _args_array)
                }
                Event::Capabilities { value } => {
                    let mut _args_array: [wl_argument; 1] = unsafe { ::std::mem::zeroed() };
                    _args_array[0].u = value;
                    f(3, &mut _args_array)
                }
            }
        }
    }
    #[derive(Clone, Eq, PartialEq)]
    pub struct WlDrm(Resource<WlDrm>);
    impl AsRef<Resource<WlDrm>> for WlDrm {
        #[inline]
        fn as_ref(&self) -> &Resource<Self> {
            &self.0
        }
    }
    impl From<Resource<WlDrm>> for WlDrm {
        #[inline]
        fn from(value: Resource<Self>) -> Self {
            WlDrm(value)
        }
    }
    impl From<WlDrm> for Resource<WlDrm> {
        #[inline]
        fn from(value: WlDrm) -> Self {
            value.0
        }
    }
    impl std::fmt::Debug for WlDrm {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_fmt(format_args!("{:?}", self.0))
        }
    }
    impl Interface for WlDrm {
        type Request = Request;
        type Event = Event;
        const NAME: &'static str = "wl_drm";
        const VERSION: u32 = 2;
        fn c_interface() -> *const wl_interface {
            unsafe { &wl_drm_interface }
        }
    }
    impl WlDrm {
        #[doc = ""]
        pub fn device(&self, name: String) -> () {
            let msg = Event::Device { name: name };
            self.0.send(msg);
        }
        #[doc = ""]
        pub fn format(&self, format: u32) -> () {
            let msg = Event::Format { format: format };
            self.0.send(msg);
        }
        #[doc = ""]
        pub fn authenticated(&self) -> () {
            let msg = Event::Authenticated;
            self.0.send(msg);
        }
        #[doc = ""]
        pub fn capabilities(&self, value: u32) -> () {
            let msg = Event::Capabilities { value: value };
            self.0.send(msg);
        }
    }
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_AUTHENTICATE_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_CREATE_BUFFER_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_CREATE_PLANAR_BUFFER_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_CREATE_PRIME_BUFFER_SINCE: u32 = 2u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_DEVICE_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_FORMAT_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_AUTHENTICATED_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_CAPABILITIES_SINCE: u32 = 1u32;
    static mut wl_drm_requests_create_buffer_types: [*const wl_interface; 6] = [
        unsafe { &super::wl_buffer::wl_buffer_interface as *const wl_interface },
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
    ];
    static mut wl_drm_requests_create_planar_buffer_types: [*const wl_interface; 11] = [
        unsafe { &super::wl_buffer::wl_buffer_interface as *const wl_interface },
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
    ];
    static mut wl_drm_requests_create_prime_buffer_types: [*const wl_interface; 11] = [
        unsafe { &super::wl_buffer::wl_buffer_interface as *const wl_interface },
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
    ];
    #[doc = r" C-representation of the messages of this interface, for interop"]
    pub static mut wl_drm_requests: [wl_message; 4] = [
        wl_message {
            name: b"authenticate\0" as *const u8 as *const c_char,
            signature: b"u\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"create_buffer\0" as *const u8 as *const c_char,
            signature: b"nuiiuu\0" as *const u8 as *const c_char,
            types: unsafe { &wl_drm_requests_create_buffer_types as *const _ },
        },
        wl_message {
            name: b"create_planar_buffer\0" as *const u8 as *const c_char,
            signature: b"nuiiuiiiiii\0" as *const u8 as *const c_char,
            types: unsafe { &wl_drm_requests_create_planar_buffer_types as *const _ },
        },
        wl_message {
            name: b"create_prime_buffer\0" as *const u8 as *const c_char,
            signature: b"2nhiiuiiiiii\0" as *const u8 as *const c_char,
            types: unsafe { &wl_drm_requests_create_prime_buffer_types as *const _ },
        },
    ];
    #[doc = r" C-representation of the messages of this interface, for interop"]
    pub static mut wl_drm_events: [wl_message; 4] = [
        wl_message {
            name: b"device\0" as *const u8 as *const c_char,
            signature: b"s\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"format\0" as *const u8 as *const c_char,
            signature: b"u\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"authenticated\0" as *const u8 as *const c_char,
            signature: b"\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"capabilities\0" as *const u8 as *const c_char,
            signature: b"u\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
    ];
    #[doc = r" C representation of this interface, for interop"]
    pub static mut wl_drm_interface: wl_interface = wl_interface {
        name: b"wl_drm\0" as *const u8 as *const c_char,
        version: 2,
        request_count: 4,
        requests: unsafe { &wl_drm_requests as *const _ },
        event_count: 4,
        events: unsafe { &wl_drm_events as *const _ },
    };
}
