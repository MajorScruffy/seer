struct Item;

trait T {
    fn valid(&self);
}

impl T for Item {
    fn valid(&self) {
        return;
    }
}

fn process(x: &Item) {
    x.valid();
}
