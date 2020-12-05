# Category5

Here we define the toplevel struct for our compositor. It spins up two
child threads for `ways` and `vkcomp` respectively and creates a
channel for them to pass atmosphere data across.

* `ways` - wayland server.
* `thundr` - a vulkan toolkit for drawing surfaces.
* `vkcomp` - the compositor. It is a window manager that translates
  windows from `ways` into `thundr` commands.
* `input` - libinput handler. Specific to `ways`, it reacts to user
  input and updates the wayland clients
* `atmosphere` - A double-buffered database shared by `ways` and `vkcomp`.
