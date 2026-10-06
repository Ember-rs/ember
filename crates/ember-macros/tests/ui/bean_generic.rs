use ember_macros::bean;

#[bean]
fn generic_provider<T>() -> String {
    let _ = std::marker::PhantomData::<T>;
    String::new()
}

fn main() {}
