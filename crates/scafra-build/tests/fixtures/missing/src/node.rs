use scafra::prelude::*;

#[derive(Default)]
pub struct Missing;

#[service]
pub struct Service {
    missing: Missing,
}
