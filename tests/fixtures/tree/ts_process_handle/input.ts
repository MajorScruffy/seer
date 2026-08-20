function process(items: Item[]) {
    if (items.length === 0) {
        console.warn("empty");
        return;
    }
    for (const item of items) {
        if (item.valid()) {
            handle(item);
        }
    }
}

function handle(item: Item) {
    if (!item.ready()) {
        return;
    }
    JSON.stringify(item);
    fs.writeFileSync(path, data);
}
