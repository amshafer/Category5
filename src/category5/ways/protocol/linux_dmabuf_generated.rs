use std::os::raw::{c_char, c_void};
const NULLPTR: *const c_void = 0 as *const c_void;
static mut types_null: [*const sys::common::wl_interface; 6] = [
    NULLPTR as *const sys::common::wl_interface,
    NULLPTR as *const sys::common::wl_interface,
    NULLPTR as *const sys::common::wl_interface,
    NULLPTR as *const sys::common::wl_interface,
    NULLPTR as *const sys::common::wl_interface,
    NULLPTR as *const sys::common::wl_interface,
];
#[doc = "factory for creating dmabuf-based wl_buffers\n\nFollowing the interfaces from:\nhttps://www.khronos.org/registry/egl/extensions/EXT/EGL_EXT_image_dma_buf_import.txt\nhttps://www.khronos.org/registry/EGL/extensions/EXT/EGL_EXT_image_dma_buf_import_modifiers.txt\nand the Linux DRM sub-system's AddFb2 ioctl.\n\nThis interface offers ways to create generic dmabuf-based wl_buffers.\n\nClients can use the get_surface_feedback request to get dmabuf feedback\nfor a particular surface. If the client wants to retrieve feedback not\ntied to a surface, they can use the get_default_feedback request.\n\nThe following are required from clients:\n\n- Clients must ensure that either all data in the dma-buf is\ncoherent for all subsequent read access or that coherency is\ncorrectly handled by the underlying kernel-side dma-buf\nimplementation.\n\n- Don't make any more attachments after sending the buffer to the\ncompositor. Making more attachments later increases the risk of\nthe compositor not being able to use (re-import) an existing\ndmabuf-based wl_buffer.\n\nThe underlying graphics stack must ensure the following:\n\n- The dmabuf file descriptors relayed to the server will stay valid\nfor the whole lifetime of the wl_buffer. This means the server may\nat any time use those fds to import the dmabuf into any kernel\nsub-system that might accept it.\n\nHowever, when the underlying graphics stack fails to deliver the\npromise, because of e.g. a device hot-unplug which raises internal\nerrors, after the wl_buffer has been successfully created the\ncompositor must not raise protocol errors to the client when dmabuf\nimport later fails.\n\nTo create a wl_buffer from one or more dmabufs, a client creates a\nzwp_linux_dmabuf_params_v1 object with a zwp_linux_dmabuf_v1.create_params\nrequest. All planes required by the intended format are added with\nthe 'add' request. Finally, a 'create' or 'create_immed' request is\nissued, which has the following outcome depending on the import success.\n\nThe 'create' request,\n- on success, triggers a 'created' event which provides the final\nwl_buffer to the client.\n- on failure, triggers a 'failed' event to convey that the server\ncannot use the dmabufs received from the client.\n\nFor the 'create_immed' request,\n- on success, the server immediately imports the added dmabufs to\ncreate a wl_buffer. No event is sent from the server in this case.\n- on failure, the server can choose to either:\n- terminate the client by raising a fatal error.\n- mark the wl_buffer as failed, and send a 'failed' event to the\nclient. If the client uses a failed wl_buffer as an argument to any\nrequest, the behaviour is compositor implementation-defined.\n\nFor all DRM formats and unless specified in another protocol extension,\npre-multiplied alpha is used for pixel values.\n\nWarning! The protocol described in this file is experimental and\nbackward incompatible changes may be made. Backward compatible changes\nmay be added together with the corresponding interface version bump.\nBackward incompatible changes are done by bumping the version number in\nthe protocol and interface names and resetting the interface version.\nOnce the protocol is to be declared stable, the 'z' prefix and the\nversion number in the protocol and interface names are removed and the\ninterface version number is reset."]
pub mod zwp_linux_dmabuf_v1 {
    use super::sys::common::{wl_argument, wl_array, wl_interface, wl_message};
    use super::sys::server::*;
    use super::{
        smallvec, types_null, AnonymousObject, Argument, ArgumentType, Interface, Main, Message,
        MessageDesc, MessageGroup, Object, ObjectMetadata, Resource, NULLPTR,
    };
    use std::os::raw::c_char;
    #[derive(Debug)]
    #[non_exhaustive]
    pub enum Request {
        #[doc = "unbind the factory\n\nObjects created through this interface, especially wl_buffers, will\nremain valid.\n\nThis is a destructor, once received this object cannot be used any longer."]
        Destroy,
        #[doc = "create a temporary object for buffer parameters\n\nThis temporary object is used to collect multiple dmabuf handles into\na single batch to create a wl_buffer. It can only be used once and\nshould be destroyed after a 'created' or 'failed' event has been\nreceived."]
        CreateParams {
            params_id: Main<super::zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1>,
        },
        #[doc = "get default feedback\n\nThis request creates a new wp_linux_dmabuf_feedback object not bound\nto a particular surface. This object will deliver feedback about dmabuf\nparameters to use if the client doesn't support per-surface feedback\n(see get_surface_feedback).\n\nOnly available since version 4 of the interface"]
        GetDefaultFeedback {
            id: Main<super::zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1>,
        },
        #[doc = "get feedback for a surface\n\nThis request creates a new wp_linux_dmabuf_feedback object for the\nspecified wl_surface. This object will deliver feedback about dmabuf\nparameters to use for buffers attached to this surface.\n\nIf the surface is destroyed before the wp_linux_dmabuf_feedback object,\nthe feedback object becomes inert.\n\nOnly available since version 4 of the interface"]
        GetSurfaceFeedback {
            id: Main<super::zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1>,
            surface: super::wl_surface::WlSurface,
        },
    }
    impl super::MessageGroup for Request {
        const MESSAGES: &'static [super::MessageDesc] = &[
            super::MessageDesc {
                name: "destroy",
                since: 1,
                signature: &[],
                destructor: true,
            },
            super::MessageDesc {
                name: "create_params",
                since: 1,
                signature: &[super::ArgumentType::NewId],
                destructor: false,
            },
            super::MessageDesc {
                name: "get_default_feedback",
                since: 4,
                signature: &[super::ArgumentType::NewId],
                destructor: false,
            },
            super::MessageDesc {
                name: "get_surface_feedback",
                since: 4,
                signature: &[super::ArgumentType::NewId, super::ArgumentType::Object],
                destructor: false,
            },
        ];
        type Map = super::ResourceMap;
        fn is_destructor(&self) -> bool {
            match *self {
                Request::Destroy => true,
                _ => false,
            }
        }
        fn opcode(&self) -> u16 {
            match *self {
                Request::Destroy => 0,
                Request::CreateParams { .. } => 1,
                Request::GetDefaultFeedback { .. } => 2,
                Request::GetSurfaceFeedback { .. } => 3,
            }
        }
        fn since(&self) -> u32 {
            match *self {
                Request::Destroy => 1,
                Request::CreateParams { .. } => 1,
                Request::GetDefaultFeedback { .. } => 4,
                Request::GetSurfaceFeedback { .. } => 4,
            }
        }
        fn child<Meta: ObjectMetadata>(
            opcode: u16,
            version: u32,
            meta: &Meta,
        ) -> Option<Object<Meta>> {
            match opcode {
                1 => Some(Object::from_interface::<
                    super::zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
                >(version, meta.child())),
                2 => Some(Object::from_interface::<
                    super::zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
                >(version, meta.child())),
                3 => Some(Object::from_interface::<
                    super::zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
                >(version, meta.child())),
                _ => None,
            }
        }
        fn from_raw(msg: Message, map: &mut Self::Map) -> Result<Self, ()> {
            match msg.opcode {
                0 => Ok(Request::Destroy),
                1 => {
                    let mut args = msg.args.into_iter();
                    Ok(Request::CreateParams {
                        params_id: {
                            if let Some(Argument::NewId(val)) = args.next() {
                                map.get_new(val).ok_or(())?
                            } else {
                                return Err(());
                            }
                        },
                    })
                }
                2 => {
                    let mut args = msg.args.into_iter();
                    Ok(Request::GetDefaultFeedback {
                        id: {
                            if let Some(Argument::NewId(val)) = args.next() {
                                map.get_new(val).ok_or(())?
                            } else {
                                return Err(());
                            }
                        },
                    })
                }
                3 => {
                    let mut args = msg.args.into_iter();
                    Ok(Request::GetSurfaceFeedback {
                        id: {
                            if let Some(Argument::NewId(val)) = args.next() {
                                map.get_new(val).ok_or(())?
                            } else {
                                return Err(());
                            }
                        },
                        surface: {
                            if let Some(Argument::Object(val)) = args.next() {
                                map.get(val).ok_or(())?.into()
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
                0 => Ok(Request::Destroy),
                1 => {
                    let _args = ::std::slice::from_raw_parts(args, 1);
                    Ok(Request::CreateParams {
                        params_id: {
                            let me = Resource::<ZwpLinuxDmabufV1>::from_c_ptr(obj as *mut _);
                            me . make_child_for :: < super :: zwp_linux_buffer_params_v1 :: ZwpLinuxBufferParamsV1 > (_args [0] . n) . unwrap ()
                        },
                    })
                }
                2 => {
                    let _args = ::std::slice::from_raw_parts(args, 1);
                    Ok(Request::GetDefaultFeedback {
                        id: {
                            let me = Resource::<ZwpLinuxDmabufV1>::from_c_ptr(obj as *mut _);
                            me . make_child_for :: < super :: zwp_linux_dmabuf_feedback_v1 :: ZwpLinuxDmabufFeedbackV1 > (_args [0] . n) . unwrap ()
                        },
                    })
                }
                3 => {
                    let _args = ::std::slice::from_raw_parts(args, 2);
                    Ok(Request::GetSurfaceFeedback {
                        id: {
                            let me = Resource::<ZwpLinuxDmabufV1>::from_c_ptr(obj as *mut _);
                            me . make_child_for :: < super :: zwp_linux_dmabuf_feedback_v1 :: ZwpLinuxDmabufFeedbackV1 > (_args [0] . n) . unwrap ()
                        },
                        surface: Resource::<super::wl_surface::WlSurface>::from_c_ptr(
                            _args[1].o as *mut _,
                        )
                        .into(),
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
        #[doc = "supported buffer format\n\nThis event advertises one buffer format that the server supports.\nAll the supported formats are advertised once when the client\nbinds to this interface. A roundtrip after binding guarantees\nthat the client has received all supported formats.\n\nFor the definition of the format codes, see the\nzwp_linux_buffer_params_v1::create request.\n\nStarting version 4, the format event is deprecated and must not be\nsent by compositors. Instead, use get_default_feedback or\nget_surface_feedback."]
        Format { format: u32 },
        #[doc = "supported buffer format modifier\n\nThis event advertises the formats that the server supports, along with\nthe modifiers supported for each format. All the supported modifiers\nfor all the supported formats are advertised once when the client\nbinds to this interface. A roundtrip after binding guarantees that\nthe client has received all supported format-modifier pairs.\n\nFor legacy support, DRM_FORMAT_MOD_INVALID (that is, modifier_hi ==\n0x00ffffff and modifier_lo == 0xffffffff) is allowed in this event.\nIt indicates that the server can support the format with an implicit\nmodifier. When a plane has DRM_FORMAT_MOD_INVALID as its modifier, it\nis as if no explicit modifier is specified. The effective modifier\nwill be derived from the dmabuf.\n\nA compositor that sends valid modifiers and DRM_FORMAT_MOD_INVALID for\na given format supports both explicit modifiers and implicit modifiers.\n\nFor the definition of the format and modifier codes, see the\nzwp_linux_buffer_params_v1::create and zwp_linux_buffer_params_v1::add\nrequests.\n\nStarting version 4, the modifier event is deprecated and must not be\nsent by compositors. Instead, use get_default_feedback or\nget_surface_feedback.\n\nOnly available since version 3 of the interface"]
        Modifier {
            format: u32,
            modifier_hi: u32,
            modifier_lo: u32,
        },
    }
    impl super::MessageGroup for Event {
        const MESSAGES: &'static [super::MessageDesc] = &[
            super::MessageDesc {
                name: "format",
                since: 1,
                signature: &[super::ArgumentType::Uint],
                destructor: false,
            },
            super::MessageDesc {
                name: "modifier",
                since: 3,
                signature: &[
                    super::ArgumentType::Uint,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Uint,
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
                Event::Format { .. } => 0,
                Event::Modifier { .. } => 1,
            }
        }
        fn since(&self) -> u32 {
            match *self {
                Event::Format { .. } => 1,
                Event::Modifier { .. } => 3,
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
                Event::Format { format } => Message {
                    sender_id: sender_id,
                    opcode: 0,
                    args: smallvec![Argument::Uint(format),],
                },
                Event::Modifier {
                    format,
                    modifier_hi,
                    modifier_lo,
                } => Message {
                    sender_id: sender_id,
                    opcode: 1,
                    args: smallvec![
                        Argument::Uint(format),
                        Argument::Uint(modifier_hi),
                        Argument::Uint(modifier_lo),
                    ],
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
                Event::Format { format } => {
                    let mut _args_array: [wl_argument; 1] = unsafe { ::std::mem::zeroed() };
                    _args_array[0].u = format;
                    f(0, &mut _args_array)
                }
                Event::Modifier {
                    format,
                    modifier_hi,
                    modifier_lo,
                } => {
                    let mut _args_array: [wl_argument; 3] = unsafe { ::std::mem::zeroed() };
                    _args_array[0].u = format;
                    _args_array[1].u = modifier_hi;
                    _args_array[2].u = modifier_lo;
                    f(1, &mut _args_array)
                }
            }
        }
    }
    #[derive(Clone, Eq, PartialEq)]
    pub struct ZwpLinuxDmabufV1(Resource<ZwpLinuxDmabufV1>);
    impl AsRef<Resource<ZwpLinuxDmabufV1>> for ZwpLinuxDmabufV1 {
        #[inline]
        fn as_ref(&self) -> &Resource<Self> {
            &self.0
        }
    }
    impl From<Resource<ZwpLinuxDmabufV1>> for ZwpLinuxDmabufV1 {
        #[inline]
        fn from(value: Resource<Self>) -> Self {
            ZwpLinuxDmabufV1(value)
        }
    }
    impl From<ZwpLinuxDmabufV1> for Resource<ZwpLinuxDmabufV1> {
        #[inline]
        fn from(value: ZwpLinuxDmabufV1) -> Self {
            value.0
        }
    }
    impl std::fmt::Debug for ZwpLinuxDmabufV1 {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_fmt(format_args!("{:?}", self.0))
        }
    }
    impl Interface for ZwpLinuxDmabufV1 {
        type Request = Request;
        type Event = Event;
        const NAME: &'static str = "zwp_linux_dmabuf_v1";
        const VERSION: u32 = 4;
        fn c_interface() -> *const wl_interface {
            unsafe { &zwp_linux_dmabuf_v1_interface }
        }
    }
    impl ZwpLinuxDmabufV1 {
        #[doc = "supported buffer format\n\nThis event advertises one buffer format that the server supports.\nAll the supported formats are advertised once when the client\nbinds to this interface. A roundtrip after binding guarantees\nthat the client has received all supported formats.\n\nFor the definition of the format codes, see the\nzwp_linux_buffer_params_v1::create request.\n\nStarting version 4, the format event is deprecated and must not be\nsent by compositors. Instead, use get_default_feedback or\nget_surface_feedback."]
        pub fn format(&self, format: u32) -> () {
            let msg = Event::Format { format: format };
            self.0.send(msg);
        }
        #[doc = "supported buffer format modifier\n\nThis event advertises the formats that the server supports, along with\nthe modifiers supported for each format. All the supported modifiers\nfor all the supported formats are advertised once when the client\nbinds to this interface. A roundtrip after binding guarantees that\nthe client has received all supported format-modifier pairs.\n\nFor legacy support, DRM_FORMAT_MOD_INVALID (that is, modifier_hi ==\n0x00ffffff and modifier_lo == 0xffffffff) is allowed in this event.\nIt indicates that the server can support the format with an implicit\nmodifier. When a plane has DRM_FORMAT_MOD_INVALID as its modifier, it\nis as if no explicit modifier is specified. The effective modifier\nwill be derived from the dmabuf.\n\nA compositor that sends valid modifiers and DRM_FORMAT_MOD_INVALID for\na given format supports both explicit modifiers and implicit modifiers.\n\nFor the definition of the format and modifier codes, see the\nzwp_linux_buffer_params_v1::create and zwp_linux_buffer_params_v1::add\nrequests.\n\nStarting version 4, the modifier event is deprecated and must not be\nsent by compositors. Instead, use get_default_feedback or\nget_surface_feedback.\n\nOnly available since version 3 of the interface."]
        pub fn modifier(&self, format: u32, modifier_hi: u32, modifier_lo: u32) -> () {
            let msg = Event::Modifier {
                format: format,
                modifier_hi: modifier_hi,
                modifier_lo: modifier_lo,
            };
            self.0.send(msg);
        }
    }
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_DESTROY_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_CREATE_PARAMS_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_GET_DEFAULT_FEEDBACK_SINCE: u32 = 4u32;
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_GET_SURFACE_FEEDBACK_SINCE: u32 = 4u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_FORMAT_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_MODIFIER_SINCE: u32 = 3u32;
    static mut zwp_linux_dmabuf_v1_requests_create_params_types: [*const wl_interface; 1] =
        [unsafe {
            &super::zwp_linux_buffer_params_v1::zwp_linux_buffer_params_v1_interface
                as *const wl_interface
        }];
    static mut zwp_linux_dmabuf_v1_requests_get_default_feedback_types: [*const wl_interface; 1] =
        [unsafe {
            &super::zwp_linux_dmabuf_feedback_v1::zwp_linux_dmabuf_feedback_v1_interface
                as *const wl_interface
        }];
    static mut zwp_linux_dmabuf_v1_requests_get_surface_feedback_types: [*const wl_interface; 2] = [
        unsafe {
            &super::zwp_linux_dmabuf_feedback_v1::zwp_linux_dmabuf_feedback_v1_interface
                as *const wl_interface
        },
        unsafe { &super::wl_surface::wl_surface_interface as *const wl_interface },
    ];
    #[doc = r" C-representation of the messages of this interface, for interop"]
    pub static mut zwp_linux_dmabuf_v1_requests: [wl_message; 4] = [
        wl_message {
            name: b"destroy\0" as *const u8 as *const c_char,
            signature: b"\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"create_params\0" as *const u8 as *const c_char,
            signature: b"n\0" as *const u8 as *const c_char,
            types: unsafe { &zwp_linux_dmabuf_v1_requests_create_params_types as *const _ },
        },
        wl_message {
            name: b"get_default_feedback\0" as *const u8 as *const c_char,
            signature: b"4n\0" as *const u8 as *const c_char,
            types: unsafe { &zwp_linux_dmabuf_v1_requests_get_default_feedback_types as *const _ },
        },
        wl_message {
            name: b"get_surface_feedback\0" as *const u8 as *const c_char,
            signature: b"4no\0" as *const u8 as *const c_char,
            types: unsafe { &zwp_linux_dmabuf_v1_requests_get_surface_feedback_types as *const _ },
        },
    ];
    #[doc = r" C-representation of the messages of this interface, for interop"]
    pub static mut zwp_linux_dmabuf_v1_events: [wl_message; 2] = [
        wl_message {
            name: b"format\0" as *const u8 as *const c_char,
            signature: b"u\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"modifier\0" as *const u8 as *const c_char,
            signature: b"3uuu\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
    ];
    #[doc = r" C representation of this interface, for interop"]
    pub static mut zwp_linux_dmabuf_v1_interface: wl_interface = wl_interface {
        name: b"zwp_linux_dmabuf_v1\0" as *const u8 as *const c_char,
        version: 4,
        request_count: 4,
        requests: unsafe { &zwp_linux_dmabuf_v1_requests as *const _ },
        event_count: 2,
        events: unsafe { &zwp_linux_dmabuf_v1_events as *const _ },
    };
}
#[doc = "parameters for creating a dmabuf-based wl_buffer\n\nThis temporary object is a collection of dmabufs and other\nparameters that together form a single logical buffer. The temporary\nobject may eventually create one wl_buffer unless cancelled by\ndestroying it before requesting 'create'.\n\nSingle-planar formats only require one dmabuf, however\nmulti-planar formats may require more than one dmabuf. For all\nformats, an 'add' request must be called once per plane (even if the\nunderlying dmabuf fd is identical).\n\nYou must use consecutive plane indices ('plane_idx' argument for 'add')\nfrom zero to the number of planes used by the drm_fourcc format code.\nAll planes required by the format must be given exactly once, but can\nbe given in any order. Each plane index can be set only once."]
pub mod zwp_linux_buffer_params_v1 {
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
        #[doc = "the dmabuf_batch object has already been used to create a wl_buffer"]
        AlreadyUsed = 0,
        #[doc = "plane index out of bounds"]
        PlaneIdx = 1,
        #[doc = "the plane index was already set"]
        PlaneSet = 2,
        #[doc = "missing or too many planes to create a buffer"]
        Incomplete = 3,
        #[doc = "format not supported"]
        InvalidFormat = 4,
        #[doc = "invalid width or height"]
        InvalidDimensions = 5,
        #[doc = "offset + stride * height goes out of dmabuf bounds"]
        OutOfBounds = 6,
        #[doc = "invalid wl_buffer resulted from importing dmabufs via the create_immed request on given buffer_params"]
        InvalidWlBuffer = 7,
    }
    impl Error {
        pub fn from_raw(n: u32) -> Option<Error> {
            match n {
                0 => Some(Error::AlreadyUsed),
                1 => Some(Error::PlaneIdx),
                2 => Some(Error::PlaneSet),
                3 => Some(Error::Incomplete),
                4 => Some(Error::InvalidFormat),
                5 => Some(Error::InvalidDimensions),
                6 => Some(Error::OutOfBounds),
                7 => Some(Error::InvalidWlBuffer),
                _ => Option::None,
            }
        }
        pub fn to_raw(&self) -> u32 {
            *self as u32
        }
    }
    bitflags! { pub struct Flags : u32 { # [doc = "contents are y-inverted"] const YInvert = 1 ; # [doc = "content is interlaced"] const Interlaced = 2 ; # [doc = "bottom field first"] const BottomFirst = 4 ; } }
    impl Flags {
        pub fn from_raw(n: u32) -> Option<Flags> {
            Some(Flags::from_bits_truncate(n))
        }
        pub fn to_raw(&self) -> u32 {
            self.bits()
        }
    }
    #[derive(Debug)]
    #[non_exhaustive]
    pub enum Request {
        #[doc = "delete this object, used or not\n\nCleans up the temporary data sent to the server for dmabuf-based\nwl_buffer creation.\n\nThis is a destructor, once received this object cannot be used any longer."]
        Destroy,
        #[doc = "add a dmabuf to the temporary set\n\nThis request adds one dmabuf to the set in this\nzwp_linux_buffer_params_v1.\n\nThe 64-bit unsigned value combined from modifier_hi and modifier_lo\nis the dmabuf layout modifier. DRM AddFB2 ioctl calls this the\nfb modifier, which is defined in drm_mode.h of Linux UAPI.\nThis is an opaque token. Drivers use this token to express tiling,\ncompression, etc. driver-specific modifications to the base format\ndefined by the DRM fourcc code.\n\nStarting from version 4, the invalid_format protocol error is sent if\nthe format + modifier pair was not advertised as supported.\n\nThis request raises the PLANE_IDX error if plane_idx is too large.\nThe error PLANE_SET is raised if attempting to set a plane that\nwas already set."]
        Add {
            fd: ::std::os::unix::io::RawFd,
            plane_idx: u32,
            offset: u32,
            stride: u32,
            modifier_hi: u32,
            modifier_lo: u32,
        },
        #[doc = "create a wl_buffer from the given dmabufs\n\nThis asks for creation of a wl_buffer from the added dmabuf\nbuffers. The wl_buffer is not created immediately but returned via\nthe 'created' event if the dmabuf sharing succeeds. The sharing\nmay fail at runtime for reasons a client cannot predict, in\nwhich case the 'failed' event is triggered.\n\nThe 'format' argument is a DRM_FORMAT code, as defined by the\nlibdrm's drm_fourcc.h. The Linux kernel's DRM sub-system is the\nauthoritative source on how the format codes should work.\n\nThe 'flags' is a bitfield of the flags defined in enum \"flags\".\n'y_invert' means the that the image needs to be y-flipped.\n\nFlag 'interlaced' means that the frame in the buffer is not\nprogressive as usual, but interlaced. An interlaced buffer as\nsupported here must always contain both top and bottom fields.\nThe top field always begins on the first pixel row. The temporal\nordering between the two fields is top field first, unless\n'bottom_first' is specified. It is undefined whether 'bottom_first'\nis ignored if 'interlaced' is not set.\n\nThis protocol does not convey any information about field rate,\nduration, or timing, other than the relative ordering between the\ntwo fields in one buffer. A compositor may have to estimate the\nintended field rate from the incoming buffer rate. It is undefined\nwhether the time of receiving wl_surface.commit with a new buffer\nattached, applying the wl_surface state, wl_surface.frame callback\ntrigger, presentation, or any other point in the compositor cycle\nis used to measure the frame or field times. There is no support\nfor detecting missed or late frames/fields/buffers either, and\nthere is no support whatsoever for cooperating with interlaced\ncompositor output.\n\nThe composited image quality resulting from the use of interlaced\nbuffers is explicitly undefined. A compositor may use elaborate\nhardware features or software to deinterlace and create progressive\noutput frames from a sequence of interlaced input buffers, or it\nmay produce substandard image quality. However, compositors that\ncannot guarantee reasonable image quality in all cases are recommended\nto just reject all interlaced buffers.\n\nAny argument errors, including non-positive width or height,\nmismatch between the number of planes and the format, bad\nformat, bad offset or stride, may be indicated by fatal protocol\nerrors: INCOMPLETE, INVALID_FORMAT, INVALID_DIMENSIONS,\nOUT_OF_BOUNDS.\n\nDmabuf import errors in the server that are not obvious client\nbugs are returned via the 'failed' event as non-fatal. This\nallows attempting dmabuf sharing and falling back in the client\nif it fails.\n\nThis request can be sent only once in the object's lifetime, after\nwhich the only legal request is destroy. This object should be\ndestroyed after issuing a 'create' request. Attempting to use this\nobject after issuing 'create' raises ALREADY_USED protocol error.\n\nIt is not mandatory to issue 'create'. If a client wants to\ncancel the buffer creation, it can just destroy this object."]
        Create {
            width: i32,
            height: i32,
            format: u32,
            flags: Flags,
        },
        #[doc = "immediately create a wl_buffer from the given dmabufs\n\nThis asks for immediate creation of a wl_buffer by importing the\nadded dmabufs.\n\nIn case of import success, no event is sent from the server, and the\nwl_buffer is ready to be used by the client.\n\nUpon import failure, either of the following may happen, as seen fit\nby the implementation:\n- the client is terminated with one of the following fatal protocol\nerrors:\n- INCOMPLETE, INVALID_FORMAT, INVALID_DIMENSIONS, OUT_OF_BOUNDS,\nin case of argument errors such as mismatch between the number\nof planes and the format, bad format, non-positive width or\nheight, or bad offset or stride.\n- INVALID_WL_BUFFER, in case the cause for failure is unknown or\nplaform specific.\n- the server creates an invalid wl_buffer, marks it as failed and\nsends a 'failed' event to the client. The result of using this\ninvalid wl_buffer as an argument in any request by the client is\ndefined by the compositor implementation.\n\nThis takes the same arguments as a 'create' request, and obeys the\nsame restrictions.\n\nOnly available since version 2 of the interface"]
        CreateImmed {
            buffer_id: Main<super::wl_buffer::WlBuffer>,
            width: i32,
            height: i32,
            format: u32,
            flags: Flags,
        },
    }
    impl super::MessageGroup for Request {
        const MESSAGES: &'static [super::MessageDesc] = &[
            super::MessageDesc {
                name: "destroy",
                since: 1,
                signature: &[],
                destructor: true,
            },
            super::MessageDesc {
                name: "add",
                since: 1,
                signature: &[
                    super::ArgumentType::Fd,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Uint,
                ],
                destructor: false,
            },
            super::MessageDesc {
                name: "create",
                since: 1,
                signature: &[
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Uint,
                ],
                destructor: false,
            },
            super::MessageDesc {
                name: "create_immed",
                since: 2,
                signature: &[
                    super::ArgumentType::NewId,
                    super::ArgumentType::Int,
                    super::ArgumentType::Int,
                    super::ArgumentType::Uint,
                    super::ArgumentType::Uint,
                ],
                destructor: false,
            },
        ];
        type Map = super::ResourceMap;
        fn is_destructor(&self) -> bool {
            match *self {
                Request::Destroy => true,
                _ => false,
            }
        }
        fn opcode(&self) -> u16 {
            match *self {
                Request::Destroy => 0,
                Request::Add { .. } => 1,
                Request::Create { .. } => 2,
                Request::CreateImmed { .. } => 3,
            }
        }
        fn since(&self) -> u32 {
            match *self {
                Request::Destroy => 1,
                Request::Add { .. } => 1,
                Request::Create { .. } => 1,
                Request::CreateImmed { .. } => 2,
            }
        }
        fn child<Meta: ObjectMetadata>(
            opcode: u16,
            version: u32,
            meta: &Meta,
        ) -> Option<Object<Meta>> {
            match opcode {
                3 => Some(Object::from_interface::<super::wl_buffer::WlBuffer>(
                    version,
                    meta.child(),
                )),
                _ => None,
            }
        }
        fn from_raw(msg: Message, map: &mut Self::Map) -> Result<Self, ()> {
            match msg.opcode {
                0 => Ok(Request::Destroy),
                1 => {
                    let mut args = msg.args.into_iter();
                    Ok(Request::Add {
                        fd: {
                            if let Some(Argument::Fd(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        plane_idx: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        offset: {
                            if let Some(Argument::Uint(val)) = args.next() {
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
                        modifier_hi: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                val
                            } else {
                                return Err(());
                            }
                        },
                        modifier_lo: {
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
                    Ok(Request::Create {
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
                        flags: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                Flags::from_raw(val).ok_or(())?
                            } else {
                                return Err(());
                            }
                        },
                    })
                }
                3 => {
                    let mut args = msg.args.into_iter();
                    Ok(Request::CreateImmed {
                        buffer_id: {
                            if let Some(Argument::NewId(val)) = args.next() {
                                map.get_new(val).ok_or(())?
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
                        flags: {
                            if let Some(Argument::Uint(val)) = args.next() {
                                Flags::from_raw(val).ok_or(())?
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
                0 => Ok(Request::Destroy),
                1 => {
                    let _args = ::std::slice::from_raw_parts(args, 6);
                    Ok(Request::Add {
                        fd: _args[0].h,
                        plane_idx: _args[1].u,
                        offset: _args[2].u,
                        stride: _args[3].u,
                        modifier_hi: _args[4].u,
                        modifier_lo: _args[5].u,
                    })
                }
                2 => {
                    let _args = ::std::slice::from_raw_parts(args, 4);
                    Ok(Request::Create {
                        width: _args[0].i,
                        height: _args[1].i,
                        format: _args[2].u,
                        flags: Flags::from_raw(_args[3].u).ok_or(())?,
                    })
                }
                3 => {
                    let _args = ::std::slice::from_raw_parts(args, 5);
                    Ok(Request::CreateImmed {
                        buffer_id: {
                            let me = Resource::<ZwpLinuxBufferParamsV1>::from_c_ptr(obj as *mut _);
                            me.make_child_for::<super::wl_buffer::WlBuffer>(_args[0].n)
                                .unwrap()
                        },
                        width: _args[1].i,
                        height: _args[2].i,
                        format: _args[3].u,
                        flags: Flags::from_raw(_args[4].u).ok_or(())?,
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
        #[doc = "buffer creation succeeded\n\nThis event indicates that the attempted buffer creation was\nsuccessful. It provides the new wl_buffer referencing the dmabuf(s).\n\nUpon receiving this event, the client should destroy the\nzlinux_dmabuf_params object."]
        Created {
            buffer: Resource<super::wl_buffer::WlBuffer>,
        },
        #[doc = "buffer creation failed\n\nThis event indicates that the attempted buffer creation has\nfailed. It usually means that one of the dmabuf constraints\nhas not been fulfilled.\n\nUpon receiving this event, the client should destroy the\nzlinux_buffer_params object."]
        Failed,
    }
    impl super::MessageGroup for Event {
        const MESSAGES: &'static [super::MessageDesc] = &[
            super::MessageDesc {
                name: "created",
                since: 1,
                signature: &[super::ArgumentType::NewId],
                destructor: false,
            },
            super::MessageDesc {
                name: "failed",
                since: 1,
                signature: &[],
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
                Event::Created { .. } => 0,
                Event::Failed => 1,
            }
        }
        fn since(&self) -> u32 {
            match *self {
                Event::Created { .. } => 1,
                Event::Failed => 1,
            }
        }
        fn child<Meta: ObjectMetadata>(
            opcode: u16,
            version: u32,
            meta: &Meta,
        ) -> Option<Object<Meta>> {
            match opcode {
                0 => Some(Object::from_interface::<super::wl_buffer::WlBuffer>(
                    version,
                    meta.child(),
                )),
                _ => None,
            }
        }
        fn from_raw(msg: Message, map: &mut Self::Map) -> Result<Self, ()> {
            panic!("Event::from_raw can not be used Server-side.")
        }
        fn into_raw(self, sender_id: u32) -> Message {
            match self {
                Event::Created { buffer } => Message {
                    sender_id: sender_id,
                    opcode: 0,
                    args: smallvec![Argument::NewId(buffer.id()),],
                },
                Event::Failed => Message {
                    sender_id: sender_id,
                    opcode: 1,
                    args: smallvec![],
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
                Event::Created { buffer } => {
                    let mut _args_array: [wl_argument; 1] = unsafe { ::std::mem::zeroed() };
                    _args_array[0].o = buffer.c_ptr() as *mut _;
                    f(0, &mut _args_array)
                }
                Event::Failed => {
                    let mut _args_array: [wl_argument; 0] = unsafe { ::std::mem::zeroed() };
                    f(1, &mut _args_array)
                }
            }
        }
    }
    #[derive(Clone, Eq, PartialEq)]
    pub struct ZwpLinuxBufferParamsV1(Resource<ZwpLinuxBufferParamsV1>);
    impl AsRef<Resource<ZwpLinuxBufferParamsV1>> for ZwpLinuxBufferParamsV1 {
        #[inline]
        fn as_ref(&self) -> &Resource<Self> {
            &self.0
        }
    }
    impl From<Resource<ZwpLinuxBufferParamsV1>> for ZwpLinuxBufferParamsV1 {
        #[inline]
        fn from(value: Resource<Self>) -> Self {
            ZwpLinuxBufferParamsV1(value)
        }
    }
    impl From<ZwpLinuxBufferParamsV1> for Resource<ZwpLinuxBufferParamsV1> {
        #[inline]
        fn from(value: ZwpLinuxBufferParamsV1) -> Self {
            value.0
        }
    }
    impl std::fmt::Debug for ZwpLinuxBufferParamsV1 {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_fmt(format_args!("{:?}", self.0))
        }
    }
    impl Interface for ZwpLinuxBufferParamsV1 {
        type Request = Request;
        type Event = Event;
        const NAME: &'static str = "zwp_linux_buffer_params_v1";
        const VERSION: u32 = 4;
        fn c_interface() -> *const wl_interface {
            unsafe { &zwp_linux_buffer_params_v1_interface }
        }
    }
    impl ZwpLinuxBufferParamsV1 {
        #[doc = "buffer creation succeeded\n\nThis event indicates that the attempted buffer creation was\nsuccessful. It provides the new wl_buffer referencing the dmabuf(s).\n\nUpon receiving this event, the client should destroy the\nzlinux_dmabuf_params object."]
        pub fn created(&self, buffer: &super::wl_buffer::WlBuffer) -> () {
            let msg = Event::Created {
                buffer: buffer.as_ref().clone(),
            };
            self.0.send(msg);
        }
        #[doc = "buffer creation failed\n\nThis event indicates that the attempted buffer creation has\nfailed. It usually means that one of the dmabuf constraints\nhas not been fulfilled.\n\nUpon receiving this event, the client should destroy the\nzlinux_buffer_params object."]
        pub fn failed(&self) -> () {
            let msg = Event::Failed;
            self.0.send(msg);
        }
    }
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_DESTROY_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_ADD_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_CREATE_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_CREATE_IMMED_SINCE: u32 = 2u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_CREATED_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_FAILED_SINCE: u32 = 1u32;
    static mut zwp_linux_buffer_params_v1_requests_create_immed_types: [*const wl_interface; 5] = [
        unsafe { &super::wl_buffer::wl_buffer_interface as *const wl_interface },
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
        NULLPTR as *const wl_interface,
    ];
    #[doc = r" C-representation of the messages of this interface, for interop"]
    pub static mut zwp_linux_buffer_params_v1_requests: [wl_message; 4] = [
        wl_message {
            name: b"destroy\0" as *const u8 as *const c_char,
            signature: b"\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"add\0" as *const u8 as *const c_char,
            signature: b"huuuuu\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"create\0" as *const u8 as *const c_char,
            signature: b"iiuu\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"create_immed\0" as *const u8 as *const c_char,
            signature: b"2niiuu\0" as *const u8 as *const c_char,
            types: unsafe { &zwp_linux_buffer_params_v1_requests_create_immed_types as *const _ },
        },
    ];
    static mut zwp_linux_buffer_params_v1_events_created_types: [*const wl_interface; 1] =
        [unsafe { &super::wl_buffer::wl_buffer_interface as *const wl_interface }];
    #[doc = r" C-representation of the messages of this interface, for interop"]
    pub static mut zwp_linux_buffer_params_v1_events: [wl_message; 2] = [
        wl_message {
            name: b"created\0" as *const u8 as *const c_char,
            signature: b"n\0" as *const u8 as *const c_char,
            types: unsafe { &zwp_linux_buffer_params_v1_events_created_types as *const _ },
        },
        wl_message {
            name: b"failed\0" as *const u8 as *const c_char,
            signature: b"\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
    ];
    #[doc = r" C representation of this interface, for interop"]
    pub static mut zwp_linux_buffer_params_v1_interface: wl_interface = wl_interface {
        name: b"zwp_linux_buffer_params_v1\0" as *const u8 as *const c_char,
        version: 4,
        request_count: 4,
        requests: unsafe { &zwp_linux_buffer_params_v1_requests as *const _ },
        event_count: 2,
        events: unsafe { &zwp_linux_buffer_params_v1_events as *const _ },
    };
}
#[doc = "dmabuf feedback\n\nThis object advertises dmabuf parameters feedback. This includes the\npreferred devices and the supported formats/modifiers.\n\nThe parameters are sent once when this object is created and whenever they\nchange. The done event is always sent once after all parameters have been\nsent. When a single parameter changes, all parameters are re-sent by the\ncompositor.\n\nCompositors can re-send the parameters when the current client buffer\nallocations are sub-optimal. Compositors should not re-send the\nparameters if re-allocating the buffers would not result in a more optimal\nconfiguration. In particular, compositors should avoid sending the exact\nsame parameters multiple times in a row.\n\nThe tranche_target_device and tranche_modifier events are grouped by\ntranches of preference. For each tranche, a tranche_target_device, one\ntranche_flags and one or more tranche_modifier events are sent, followed\nby a tranche_done event finishing the list. The tranches are sent in\ndescending order of preference. All formats and modifiers in the same\ntranche have the same preference.\n\nTo send parameters, the compositor sends one main_device event, tranches\n(each consisting of one tranche_target_device event, one tranche_flags\nevent, tranche_modifier events and then a tranche_done event), then one\ndone event."]
pub mod zwp_linux_dmabuf_feedback_v1 {
    use super::sys::common::{wl_argument, wl_array, wl_interface, wl_message};
    use super::sys::server::*;
    use super::{
        smallvec, types_null, AnonymousObject, Argument, ArgumentType, Interface, Main, Message,
        MessageDesc, MessageGroup, Object, ObjectMetadata, Resource, NULLPTR,
    };
    use std::os::raw::c_char;
    bitflags! { pub struct TrancheFlags : u32 { # [doc = "direct scan-out tranche"] const Scanout = 1 ; } }
    impl TrancheFlags {
        pub fn from_raw(n: u32) -> Option<TrancheFlags> {
            Some(TrancheFlags::from_bits_truncate(n))
        }
        pub fn to_raw(&self) -> u32 {
            self.bits()
        }
    }
    #[derive(Debug)]
    #[non_exhaustive]
    pub enum Request {
        #[doc = "destroy the feedback object\n\nUsing this request a client can tell the server that it is not going to\nuse the wp_linux_dmabuf_feedback object anymore.\n\nThis is a destructor, once received this object cannot be used any longer."]
        Destroy,
    }
    impl super::MessageGroup for Request {
        const MESSAGES: &'static [super::MessageDesc] = &[super::MessageDesc {
            name: "destroy",
            since: 1,
            signature: &[],
            destructor: true,
        }];
        type Map = super::ResourceMap;
        fn is_destructor(&self) -> bool {
            match *self {
                Request::Destroy => true,
            }
        }
        fn opcode(&self) -> u16 {
            match *self {
                Request::Destroy => 0,
            }
        }
        fn since(&self) -> u32 {
            match *self {
                Request::Destroy => 1,
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
            match msg.opcode {
                0 => Ok(Request::Destroy),
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
                0 => Ok(Request::Destroy),
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
        #[doc = "all feedback has been sent\n\nThis event is sent after all parameters of a wp_linux_dmabuf_feedback\nobject have been sent.\n\nThis allows changes to the wp_linux_dmabuf_feedback parameters to be\nseen as atomic, even if they happen via multiple events."]
        Done,
        #[doc = "format and modifier table\n\nThis event provides a file descriptor which can be memory-mapped to\naccess the format and modifier table.\n\nThe table contains a tightly packed array of consecutive format +\nmodifier pairs. Each pair is 16 bytes wide. It contains a format as a\n32-bit unsigned integer, followed by 4 bytes of unused padding, and a\nmodifier as a 64-bit unsigned integer. The native endianness is used.\n\nThe client must map the file descriptor in read-only private mode.\n\nCompositors are not allowed to mutate the table file contents once this\nevent has been sent. Instead, compositors must create a new, separate\ntable file and re-send feedback parameters. Compositors are allowed to\nstore duplicate format + modifier pairs in the table."]
        FormatTable {
            fd: ::std::os::unix::io::RawFd,
            size: u32,
        },
        #[doc = "preferred main device\n\nThis event advertises the main device that the server prefers to use\nwhen direct scan-out to the target device isn't possible. The\nadvertised main device may be different for each\nwp_linux_dmabuf_feedback object, and may change over time.\n\nThere is exactly one main device. The compositor must send at least\none preference tranche with tranche_target_device equal to main_device.\n\nClients need to create buffers that the main device can import and\nread from, otherwise creating the dmabuf wl_buffer will fail (see the\nwp_linux_buffer_params.create and create_immed requests for details).\nThe main device will also likely be kept active by the compositor,\nso clients can use it instead of waking up another device for power\nsavings.\n\nIn general the device is a DRM node. The DRM node type (primary vs.\nrender) is unspecified. Clients must not rely on the compositor sending\na particular node type. Clients cannot check two devices for equality\nby comparing the dev_t value.\n\nIf explicit modifiers are not supported and the client performs buffer\nallocations on a different device than the main device, then the client\nmust force the buffer to have a linear layout."]
        MainDevice { device: Vec<u8> },
        #[doc = "a preference tranche has been sent\n\nThis event splits tranche_target_device and tranche_modifier events in\npreference tranches. It is sent after a set of tranche_target_device\nand tranche_modifier events; it represents the end of a tranche. The\nnext tranche will have a lower preference."]
        TrancheDone,
        #[doc = "target device\n\nThis event advertises the target device that the server prefers to use\nfor a buffer created given this tranche. The advertised target device\nmay be different for each preference tranche, and may change over time.\n\nThere is exactly one target device per tranche.\n\nThe target device may be a scan-out device, for example if the\ncompositor prefers to directly scan-out a buffer created given this\ntranche. The target device may be a rendering device, for example if\nthe compositor prefers to texture from said buffer.\n\nThe client can use this hint to allocate the buffer in a way that makes\nit accessible from the target device, ideally directly. The buffer must\nstill be accessible from the main device, either through direct import\nor through a potentially more expensive fallback path. If the buffer\ncan't be directly imported from the main device then clients must be\nprepared for the compositor changing the tranche priority or making\nwl_buffer creation fail (see the wp_linux_buffer_params.create and\ncreate_immed requests for details).\n\nIf the device is a DRM node, the DRM node type (primary vs. render) is\nunspecified. Clients must not rely on the compositor sending a\nparticular node type. Clients cannot check two devices for equality by\ncomparing the dev_t value.\n\nThis event is tied to a preference tranche, see the tranche_done event."]
        TrancheTargetDevice { device: Vec<u8> },
        #[doc = "supported buffer format modifier\n\nThis event advertises the format + modifier combinations that the\ncompositor supports.\n\nIt carries an array of indices, each referring to a format + modifier\npair in the last received format table (see the format_table event).\nEach index is a 16-bit unsigned integer in native endianness.\n\nFor legacy support, DRM_FORMAT_MOD_INVALID is an allowed modifier.\nIt indicates that the server can support the format with an implicit\nmodifier. When a buffer has DRM_FORMAT_MOD_INVALID as its modifier, it\nis as if no explicit modifier is specified. The effective modifier\nwill be derived from the dmabuf.\n\nA compositor that sends valid modifiers and DRM_FORMAT_MOD_INVALID for\na given format supports both explicit modifiers and implicit modifiers.\n\nCompositors must not send duplicate format + modifier pairs within the\nsame tranche or across two different tranches with the same target\ndevice and flags.\n\nThis event is tied to a preference tranche, see the tranche_done event.\n\nFor the definition of the format and modifier codes, see the\nwp_linux_buffer_params.create request."]
        TrancheFormats { indices: Vec<u8> },
        #[doc = "tranche flags\n\nThis event sets tranche-specific flags.\n\nThe scanout flag is a hint that direct scan-out may be attempted by the\ncompositor on the target device if the client appropriately allocates a\nbuffer. How to allocate a buffer that can be scanned out on the target\ndevice is implementation-defined.\n\nThis event is tied to a preference tranche, see the tranche_done event."]
        TrancheFlags { flags: TrancheFlags },
    }
    impl super::MessageGroup for Event {
        const MESSAGES: &'static [super::MessageDesc] = &[
            super::MessageDesc {
                name: "done",
                since: 1,
                signature: &[],
                destructor: false,
            },
            super::MessageDesc {
                name: "format_table",
                since: 1,
                signature: &[super::ArgumentType::Fd, super::ArgumentType::Uint],
                destructor: false,
            },
            super::MessageDesc {
                name: "main_device",
                since: 1,
                signature: &[super::ArgumentType::Array],
                destructor: false,
            },
            super::MessageDesc {
                name: "tranche_done",
                since: 1,
                signature: &[],
                destructor: false,
            },
            super::MessageDesc {
                name: "tranche_target_device",
                since: 1,
                signature: &[super::ArgumentType::Array],
                destructor: false,
            },
            super::MessageDesc {
                name: "tranche_formats",
                since: 1,
                signature: &[super::ArgumentType::Array],
                destructor: false,
            },
            super::MessageDesc {
                name: "tranche_flags",
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
                Event::Done => 0,
                Event::FormatTable { .. } => 1,
                Event::MainDevice { .. } => 2,
                Event::TrancheDone => 3,
                Event::TrancheTargetDevice { .. } => 4,
                Event::TrancheFormats { .. } => 5,
                Event::TrancheFlags { .. } => 6,
            }
        }
        fn since(&self) -> u32 {
            match *self {
                Event::Done => 1,
                Event::FormatTable { .. } => 1,
                Event::MainDevice { .. } => 1,
                Event::TrancheDone => 1,
                Event::TrancheTargetDevice { .. } => 1,
                Event::TrancheFormats { .. } => 1,
                Event::TrancheFlags { .. } => 1,
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
                Event::Done => Message {
                    sender_id: sender_id,
                    opcode: 0,
                    args: smallvec![],
                },
                Event::FormatTable { fd, size } => Message {
                    sender_id: sender_id,
                    opcode: 1,
                    args: smallvec![Argument::Fd(fd), Argument::Uint(size),],
                },
                Event::MainDevice { device } => Message {
                    sender_id: sender_id,
                    opcode: 2,
                    args: smallvec![Argument::Array(Box::new(device)),],
                },
                Event::TrancheDone => Message {
                    sender_id: sender_id,
                    opcode: 3,
                    args: smallvec![],
                },
                Event::TrancheTargetDevice { device } => Message {
                    sender_id: sender_id,
                    opcode: 4,
                    args: smallvec![Argument::Array(Box::new(device)),],
                },
                Event::TrancheFormats { indices } => Message {
                    sender_id: sender_id,
                    opcode: 5,
                    args: smallvec![Argument::Array(Box::new(indices)),],
                },
                Event::TrancheFlags { flags } => Message {
                    sender_id: sender_id,
                    opcode: 6,
                    args: smallvec![Argument::Uint(flags.to_raw()),],
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
                Event::Done => {
                    let mut _args_array: [wl_argument; 0] = unsafe { ::std::mem::zeroed() };
                    f(0, &mut _args_array)
                }
                Event::FormatTable { fd, size } => {
                    let mut _args_array: [wl_argument; 2] = unsafe { ::std::mem::zeroed() };
                    _args_array[0].h = fd;
                    _args_array[1].u = size;
                    f(1, &mut _args_array)
                }
                Event::MainDevice { device } => {
                    let mut _args_array: [wl_argument; 1] = unsafe { ::std::mem::zeroed() };
                    let _arg_0 = wl_array {
                        size: device.len(),
                        alloc: device.capacity(),
                        data: device.as_ptr() as *mut _,
                    };
                    _args_array[0].a = &_arg_0;
                    f(2, &mut _args_array)
                }
                Event::TrancheDone => {
                    let mut _args_array: [wl_argument; 0] = unsafe { ::std::mem::zeroed() };
                    f(3, &mut _args_array)
                }
                Event::TrancheTargetDevice { device } => {
                    let mut _args_array: [wl_argument; 1] = unsafe { ::std::mem::zeroed() };
                    let _arg_0 = wl_array {
                        size: device.len(),
                        alloc: device.capacity(),
                        data: device.as_ptr() as *mut _,
                    };
                    _args_array[0].a = &_arg_0;
                    f(4, &mut _args_array)
                }
                Event::TrancheFormats { indices } => {
                    let mut _args_array: [wl_argument; 1] = unsafe { ::std::mem::zeroed() };
                    let _arg_0 = wl_array {
                        size: indices.len(),
                        alloc: indices.capacity(),
                        data: indices.as_ptr() as *mut _,
                    };
                    _args_array[0].a = &_arg_0;
                    f(5, &mut _args_array)
                }
                Event::TrancheFlags { flags } => {
                    let mut _args_array: [wl_argument; 1] = unsafe { ::std::mem::zeroed() };
                    _args_array[0].u = flags.to_raw();
                    f(6, &mut _args_array)
                }
            }
        }
    }
    #[derive(Clone, Eq, PartialEq)]
    pub struct ZwpLinuxDmabufFeedbackV1(Resource<ZwpLinuxDmabufFeedbackV1>);
    impl AsRef<Resource<ZwpLinuxDmabufFeedbackV1>> for ZwpLinuxDmabufFeedbackV1 {
        #[inline]
        fn as_ref(&self) -> &Resource<Self> {
            &self.0
        }
    }
    impl From<Resource<ZwpLinuxDmabufFeedbackV1>> for ZwpLinuxDmabufFeedbackV1 {
        #[inline]
        fn from(value: Resource<Self>) -> Self {
            ZwpLinuxDmabufFeedbackV1(value)
        }
    }
    impl From<ZwpLinuxDmabufFeedbackV1> for Resource<ZwpLinuxDmabufFeedbackV1> {
        #[inline]
        fn from(value: ZwpLinuxDmabufFeedbackV1) -> Self {
            value.0
        }
    }
    impl std::fmt::Debug for ZwpLinuxDmabufFeedbackV1 {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_fmt(format_args!("{:?}", self.0))
        }
    }
    impl Interface for ZwpLinuxDmabufFeedbackV1 {
        type Request = Request;
        type Event = Event;
        const NAME: &'static str = "zwp_linux_dmabuf_feedback_v1";
        const VERSION: u32 = 4;
        fn c_interface() -> *const wl_interface {
            unsafe { &zwp_linux_dmabuf_feedback_v1_interface }
        }
    }
    impl ZwpLinuxDmabufFeedbackV1 {
        #[doc = "all feedback has been sent\n\nThis event is sent after all parameters of a wp_linux_dmabuf_feedback\nobject have been sent.\n\nThis allows changes to the wp_linux_dmabuf_feedback parameters to be\nseen as atomic, even if they happen via multiple events."]
        pub fn done(&self) -> () {
            let msg = Event::Done;
            self.0.send(msg);
        }
        #[doc = "format and modifier table\n\nThis event provides a file descriptor which can be memory-mapped to\naccess the format and modifier table.\n\nThe table contains a tightly packed array of consecutive format +\nmodifier pairs. Each pair is 16 bytes wide. It contains a format as a\n32-bit unsigned integer, followed by 4 bytes of unused padding, and a\nmodifier as a 64-bit unsigned integer. The native endianness is used.\n\nThe client must map the file descriptor in read-only private mode.\n\nCompositors are not allowed to mutate the table file contents once this\nevent has been sent. Instead, compositors must create a new, separate\ntable file and re-send feedback parameters. Compositors are allowed to\nstore duplicate format + modifier pairs in the table."]
        pub fn format_table(&self, fd: ::std::os::unix::io::RawFd, size: u32) -> () {
            let msg = Event::FormatTable { fd: fd, size: size };
            self.0.send(msg);
        }
        #[doc = "preferred main device\n\nThis event advertises the main device that the server prefers to use\nwhen direct scan-out to the target device isn't possible. The\nadvertised main device may be different for each\nwp_linux_dmabuf_feedback object, and may change over time.\n\nThere is exactly one main device. The compositor must send at least\none preference tranche with tranche_target_device equal to main_device.\n\nClients need to create buffers that the main device can import and\nread from, otherwise creating the dmabuf wl_buffer will fail (see the\nwp_linux_buffer_params.create and create_immed requests for details).\nThe main device will also likely be kept active by the compositor,\nso clients can use it instead of waking up another device for power\nsavings.\n\nIn general the device is a DRM node. The DRM node type (primary vs.\nrender) is unspecified. Clients must not rely on the compositor sending\na particular node type. Clients cannot check two devices for equality\nby comparing the dev_t value.\n\nIf explicit modifiers are not supported and the client performs buffer\nallocations on a different device than the main device, then the client\nmust force the buffer to have a linear layout."]
        pub fn main_device(&self, device: Vec<u8>) -> () {
            let msg = Event::MainDevice { device: device };
            self.0.send(msg);
        }
        #[doc = "a preference tranche has been sent\n\nThis event splits tranche_target_device and tranche_modifier events in\npreference tranches. It is sent after a set of tranche_target_device\nand tranche_modifier events; it represents the end of a tranche. The\nnext tranche will have a lower preference."]
        pub fn tranche_done(&self) -> () {
            let msg = Event::TrancheDone;
            self.0.send(msg);
        }
        #[doc = "target device\n\nThis event advertises the target device that the server prefers to use\nfor a buffer created given this tranche. The advertised target device\nmay be different for each preference tranche, and may change over time.\n\nThere is exactly one target device per tranche.\n\nThe target device may be a scan-out device, for example if the\ncompositor prefers to directly scan-out a buffer created given this\ntranche. The target device may be a rendering device, for example if\nthe compositor prefers to texture from said buffer.\n\nThe client can use this hint to allocate the buffer in a way that makes\nit accessible from the target device, ideally directly. The buffer must\nstill be accessible from the main device, either through direct import\nor through a potentially more expensive fallback path. If the buffer\ncan't be directly imported from the main device then clients must be\nprepared for the compositor changing the tranche priority or making\nwl_buffer creation fail (see the wp_linux_buffer_params.create and\ncreate_immed requests for details).\n\nIf the device is a DRM node, the DRM node type (primary vs. render) is\nunspecified. Clients must not rely on the compositor sending a\nparticular node type. Clients cannot check two devices for equality by\ncomparing the dev_t value.\n\nThis event is tied to a preference tranche, see the tranche_done event."]
        pub fn tranche_target_device(&self, device: Vec<u8>) -> () {
            let msg = Event::TrancheTargetDevice { device: device };
            self.0.send(msg);
        }
        #[doc = "supported buffer format modifier\n\nThis event advertises the format + modifier combinations that the\ncompositor supports.\n\nIt carries an array of indices, each referring to a format + modifier\npair in the last received format table (see the format_table event).\nEach index is a 16-bit unsigned integer in native endianness.\n\nFor legacy support, DRM_FORMAT_MOD_INVALID is an allowed modifier.\nIt indicates that the server can support the format with an implicit\nmodifier. When a buffer has DRM_FORMAT_MOD_INVALID as its modifier, it\nis as if no explicit modifier is specified. The effective modifier\nwill be derived from the dmabuf.\n\nA compositor that sends valid modifiers and DRM_FORMAT_MOD_INVALID for\na given format supports both explicit modifiers and implicit modifiers.\n\nCompositors must not send duplicate format + modifier pairs within the\nsame tranche or across two different tranches with the same target\ndevice and flags.\n\nThis event is tied to a preference tranche, see the tranche_done event.\n\nFor the definition of the format and modifier codes, see the\nwp_linux_buffer_params.create request."]
        pub fn tranche_formats(&self, indices: Vec<u8>) -> () {
            let msg = Event::TrancheFormats { indices: indices };
            self.0.send(msg);
        }
        #[doc = "tranche flags\n\nThis event sets tranche-specific flags.\n\nThe scanout flag is a hint that direct scan-out may be attempted by the\ncompositor on the target device if the client appropriately allocates a\nbuffer. How to allocate a buffer that can be scanned out on the target\ndevice is implementation-defined.\n\nThis event is tied to a preference tranche, see the tranche_done event."]
        pub fn tranche_flags(&self, flags: TrancheFlags) -> () {
            let msg = Event::TrancheFlags { flags: flags };
            self.0.send(msg);
        }
    }
    #[doc = r" The minimal object version supporting this request"]
    pub const REQ_DESTROY_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_DONE_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_FORMAT_TABLE_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_MAIN_DEVICE_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_TRANCHE_DONE_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_TRANCHE_TARGET_DEVICE_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_TRANCHE_FORMATS_SINCE: u32 = 1u32;
    #[doc = r" The minimal object version supporting this event"]
    pub const EVT_TRANCHE_FLAGS_SINCE: u32 = 1u32;
    #[doc = r" C-representation of the messages of this interface, for interop"]
    pub static mut zwp_linux_dmabuf_feedback_v1_requests: [wl_message; 1] = [wl_message {
        name: b"destroy\0" as *const u8 as *const c_char,
        signature: b"\0" as *const u8 as *const c_char,
        types: unsafe { &types_null as *const _ },
    }];
    #[doc = r" C-representation of the messages of this interface, for interop"]
    pub static mut zwp_linux_dmabuf_feedback_v1_events: [wl_message; 7] = [
        wl_message {
            name: b"done\0" as *const u8 as *const c_char,
            signature: b"\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"format_table\0" as *const u8 as *const c_char,
            signature: b"hu\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"main_device\0" as *const u8 as *const c_char,
            signature: b"a\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"tranche_done\0" as *const u8 as *const c_char,
            signature: b"\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"tranche_target_device\0" as *const u8 as *const c_char,
            signature: b"a\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"tranche_formats\0" as *const u8 as *const c_char,
            signature: b"a\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
        wl_message {
            name: b"tranche_flags\0" as *const u8 as *const c_char,
            signature: b"u\0" as *const u8 as *const c_char,
            types: unsafe { &types_null as *const _ },
        },
    ];
    #[doc = r" C representation of this interface, for interop"]
    pub static mut zwp_linux_dmabuf_feedback_v1_interface: wl_interface = wl_interface {
        name: b"zwp_linux_dmabuf_feedback_v1\0" as *const u8 as *const c_char,
        version: 4,
        request_count: 1,
        requests: unsafe { &zwp_linux_dmabuf_feedback_v1_requests as *const _ },
        event_count: 7,
        events: unsafe { &zwp_linux_dmabuf_feedback_v1_events as *const _ },
    };
}
