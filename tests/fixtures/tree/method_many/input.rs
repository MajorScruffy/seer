struct A;
struct B;

impl A {
    fn valid(&self) {}
}

impl B {
    fn valid(&self) {}
}

fn process(a: &A) {
    a.valid();
}
