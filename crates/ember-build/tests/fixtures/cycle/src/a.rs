use ember::prelude::*;
use crate::b::B;

#[service]
pub struct A {
    b: B,
}
