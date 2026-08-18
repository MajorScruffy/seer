struct Item;

impl Item {
    fn valid(&self) {
        return;
    }
}

fn process(item: &Item) {
    Item::valid(item);
}
