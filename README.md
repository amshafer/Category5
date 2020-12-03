# Category5 Desktop Environment

Category5 is a wayland desktop environment for FreeBSD that is focused on
performance and security. It is written in Rust, and uses the Vulkan
graphics api to perform rendering.

### Capabilities
* floating window manager
* dmabuf and shm buffer sharing with clients

### Design Goals

The primary motivations for Category5 are making a desktop that is
easy to use and fun to modify. The goal is to provide advanced
security without sacrificing performance. Rust embodies both of these
goals, wayland introduces increased isolation of applications, and
Vulkan provides platform agnostic rendering power.

The source code is heavily commented, and documentation accompanies
every part of the system. Category5 is essentially designed like a
game engine, where different subsystems work together to present the
user with a cohesive experience. The subsystems are as follows:

* `ways` - wayland server.
* `thundr` - a vulkan toolkit for drawing surfaces.
* `vkcomp` - the compositor. It is a window manager that translates
  windows from `ways` into `thundr` commands.
* `input` - libinput handler. Specific to `ways`, it reacts to user
  input and updates the wayland clients
* `atmosphere` - A double-buffered database shared by `ways` and `vkcomp`.

Another goal is to keep rendering entirely separate from the wayland
server. `vkcomp` is run in a different thread from `ways`, and
`atmosphere` serves as a barrier between the two. `ways` gets updates
from clients, posts them to `atmosphere`, and `vkcomp` constructs a frame
out of the current databse contents and presents it to the user's
display.

The reason for this is to increase security and
functionality in the future. `vkcomp` could be moved to a separate process with
different permissions than `ways`. vkcomp could also be restarted
independent of `ways` and without restarting wayland clients.

A non-goal is to make Category5 perfectly configurable. There are many
other highly customizable desktop environments, and this is not one of
them. Basic customizability for aestethics and keyboard shortcuts is
planned.

## Compiling

Category5 is supported on FreeBSD 12+.

The following dependencies are required:
* rustc/cargo
* vulkan 1.2+
* graphics drivers (nvidia-driver or mesa)
* libinput
* libudev
* wayland-protocols
* xkbcommon

## Running

The user you run Category5 as must be able to:
* access the gpu (video group)
* access the input peripherals

Simply run:
```
cargo run
```