use ember::prelude::*;
use crate::a::A;

#[service]
pub struct B {
    a: A,
}
