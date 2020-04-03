// Utils for working with wayland (not bindings)
//
// Austin Shafer - 2020

// Gets a private struct from a wl_resource
//
// wl_resources have a "user data" section which holds a private
// struct for us. This macro provides a safe and ergonomic way to grab
// that struct. The userdata will always have a container which holds
// our private struct, for now it is a RefCell. This macro "checks out"
// the private struct from its container to keep the borrow checker
// happy and our code safe.
//
// This macro uses unsafe code
//
// Example usage:
//      (get a reference to a `Surface` struct)
//  let mut surface = get_userdata!(resource, Surface).unwrap();
//
// Arguments:
//  resource: *mut wl_resource
//  generic: the type of private struct
//
// Returns:
//  Option holding the RefMut we can access the struct through
#[macro_export]
macro_rules! get_userdata_of_type {
    // We need to know what type to use for the RefCell
    ($resource:expr, $generic:ty) => {
        unsafe {
            // use .as_mut to get an option<&> we can match against
            match (wl_resource_get_user_data($resource)
                   as *mut RefCell<$generic>).as_mut() {
                None => None,
                // Borrowing from the refcell will dynamically enforce
                // lifetime contracts. This can panic.
                Some(cell) => Some((*cell).borrow_mut()),
            }
        }
    }
}

#[macro_export]
macro_rules! get_userdata_raw {
    // We need to know what type to use for the RefCell
    ($resource:expr, $generic:ty) => {
        unsafe {
            // use .as_mut to get an option<&> we can match against
            (wl_resource_get_user_data($resource)
             as *mut RefCell<$generic>).as_mut()
        }
    }
}

#[macro_export]
macro_rules! as_mut_c_void {
    ($data:expr) => {
        &mut $data as *mut _ as *mut c_void
    }
}
