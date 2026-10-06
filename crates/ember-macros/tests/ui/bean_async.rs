use ember_macros::bean;

#[bean]
async fn asynchronous_provider() -> String {
    String::new()
}

fn main() {}
