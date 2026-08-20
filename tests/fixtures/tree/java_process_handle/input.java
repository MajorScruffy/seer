class App {
    void process(Item[] items) {
        if (items.length == 0) {
            log.warn("empty");
            return;
        }
        for (Item item : items) {
            if (item.valid()) {
                handle(item);
            }
        }
    }

    void handle(Item item) {
        if (!item.ready()) {
            return;
        }
        JSON.toString(item);
        Files.write(path, data);
    }
}
