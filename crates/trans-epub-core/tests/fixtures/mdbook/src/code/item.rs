use std::fmt;

struct Widget {
    name: String,
}

impl Widget {
    fn new(name: String) -> Self {
        Self { name }
    }
}
