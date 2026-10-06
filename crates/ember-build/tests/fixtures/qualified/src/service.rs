use ember::prelude::*;

#[service]
pub struct Service {
    dependency: crate::dependency::Dependency,
}
