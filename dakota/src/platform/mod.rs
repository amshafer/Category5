use crate::dom;
use crate::{Context, Result};

#[cfg(feature = "wayland")]
extern crate wayc;
#[cfg(feature = "wayland")]
use wayc::Wayc;

#[cfg(any(unix, macos))]
extern crate winit;
#[cfg(any(unix, macos))]
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

pub trait Platform {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType>;

    fn set_output_params(&mut self, win: &dom::Window) -> Result<()>;

    fn run<F: FnMut()>(&mut self, func: F) -> Result<()>;
}

#[cfg(feature = "wayland")]
pub struct WLPlat {
    wp_wayc: Wayc,
}

#[cfg(feature = "wayland")]
impl WLPlat {
    fn new() -> Result<Self> {
        let mut wayc = Wayc::new().context("Failed to initialize wayland")?;
        let wl_surf = wayc
            .create_surface()
            .context("Failed to create wayland surface")?;

        Self {
            wp_wayc: wayc,
            wp_surf: wl_surf,
        }
    }
    fn set_output_params(&mut self, win: &dom::Window) -> Result<()> {
        println!("set_output_params on wayland is unimplemented");
    }
}

#[cfg(feature = "wayland")]
impl Platform for WLPlat {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType> {
        Ok(th::SurfaceType::Wayland(
            self.wp_wayc.get_wl_display(),
            self.wp_surf.borrow().get_wl_surface().detach(),
        ))
    }
}

#[cfg(feature = "macos")]
pub struct MacosPlat {
    mp_event_loop: EventLoop<()>,
    mp_window: winit::window::Window,
}

#[cfg(feature = "macos")]
impl MacosPlat {
    pub fn new() -> Result<Self> {
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new()
            .with_inner_size(winit::dpi::PhysicalSize::new(512, 512))
            .build(&event_loop)
            .context("Could not create window with winit")?;

        Ok(Self {
            mp_event_loop: event_loop,
            mp_window: window,
        })
    }
}

#[cfg(feature = "macos")]
impl Platform for MacosPlat {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType> {
        Ok(th::SurfaceType::MacOS(&self.mp_window))
    }

    fn set_output_params(&mut self, win: &dom::Window) -> Result<()> {
        self.mp_window.set_title(&win.title);
        // BUG: For unknown reasons, this seems to have no effect?
        self.mp_window
            .set_inner_size(winit::dpi::PhysicalSize::new(win.width, win.height));
        Ok(())
    }

    fn run<F>(&mut self, mut func: F) -> Result<()>
    where
        F: FnMut(),
    {
        use winit::platform::run_return::EventLoopExtRunReturn;

        let evloop = &mut self.mp_event_loop;
        let window = &mut self.mp_window;

        evloop.run_return(|event, _, control_flow| {
            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                    _ => (),
                },
                Event::RedrawRequested(_) => {
                    func();
                    window.request_redraw();
                    // Draw one frame and then return
                    *control_flow = ControlFlow::Exit;
                }
                _ => (),
            };
        });
        Ok(())
    }
}

#[cfg(feature = "xcb")]
pub struct XCBPlat {
    xp_event_loop: EventLoop<()>,
    xp_window: winit::window::Window,
}

#[cfg(feature = "xcb")]
impl XCBPlat {
    pub fn new() -> Result<Self> {
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new()
            .build(&event_loop)
            .context("Could not create window with winit")?;

        Ok(Self {
            xp_event_loop: event_loop,
            xp_window: window,
        })
    }
}

#[cfg(feature = "xcb")]
impl Platform for XCBPlat {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType> {
        Ok(th::SurfaceType::Xcb(&self.xp_window))
    }

    fn set_output_params(&mut self, win: &dom::Window) -> Result<()> {
        self.xp_window.set_title(&win.title);
        self.xp_window
            .set_inner_size(winit::dpi::PhysicalSize::new(win.width, win.height));
        Ok(())
    }

    fn run<F>(&mut self, mut func: F) -> Result<()>
    where
        F: FnMut(),
    {
        use winit::platform::run_return::EventLoopExtRunReturn;

        let evloop = &mut self.xp_event_loop;
        let window = &mut self.xp_window;

        evloop.run_return(|event, _, control_flow| {
            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                    _ => (),
                },
                Event::RedrawRequested(_) => {
                    func();
                    window.request_redraw();
                    // Draw one frame and then return
                    *control_flow = ControlFlow::Exit;
                }
                _ => (),
            };
        });
        Ok(())
    }
}
