use crate::second::Shared;
use scafra::prelude::*;

#[service]
pub struct Service {
    shared: Shared,
}
