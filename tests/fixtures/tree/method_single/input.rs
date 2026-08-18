struct Item;

impl Item {
    fn valid(&self) -> bool {
        return true;
    }
}

fn process(item: &Item) {
    if item.ok() {
        item.valid();
    }
}
