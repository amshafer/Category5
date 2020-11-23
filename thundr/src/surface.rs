/// A `Surface` represents a region that will be drawn on the target.
/// Surfaces have `Image`s bound to them, which will be used for compositing
/// the final frame.
///
/// Essentially the surface is the geometrical region on the screen, and the image
/// contents will be sampled into the surface's rectangle.
// Austin Shafer - 2020
#![allow(dead_code)]
extern crate nix;
extern crate ash;

use crate::{Renderer,RecordParams,PushConstants};
use cat5_utils::log_prelude::*;
use utils::{WindowContents,MemImage,Dmabuf};

use std::{mem,fmt,iter};
use std::ops::Drop;

use nix::Error;
use nix::errno::Errno;
use nix::unistd::dup;
use ash::version::{DeviceV1_0,InstanceV1_1};
use ash::vk;
