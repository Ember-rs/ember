use scafra::prelude::*;
use crate::b::B;

#[service]
pub struct A {
    b: B,
}
