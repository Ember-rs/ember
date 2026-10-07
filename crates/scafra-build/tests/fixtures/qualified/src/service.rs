use scafra::prelude::*;

#[service]
pub struct Service {
    dependency: crate::dependency::Dependency,
}
