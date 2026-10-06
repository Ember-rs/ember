use crate::second::Shared;
use ember::prelude::*;

#[service]
pub struct Service {
    shared: Shared,
}
