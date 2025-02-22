#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused)]
#![deny(clippy::missing_safety_doc)]

mod bindings;
mod msg_future;

pub use msg_future::MWMO_QUEUEATTACH;
pub use msg_future::wait_for_messages;
