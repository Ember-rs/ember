use ember_macros::Config;

#[derive(Config)]
#[config(prefix = "first", prefix = "second")]
struct InvalidConfig {
    value: String,
}

fn main() {}
