fn process(items: &[Item]) {
    if items.is_empty() {
        log::warn("empty");
        return;
    }
    for item in items {
        if item.valid() {
            handle(item);
        }
    }
}

fn handle(item: &Item) {
    if !item.ready() {
        return;
    }
    serde_json::to_string(item);
    std::fs::write(path, data);
}
