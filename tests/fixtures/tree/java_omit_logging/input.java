class C {
    void info() {
        return;
    }

    void f() {
        System.out.println("x");
        System.err.print("y");
        log.warn("empty");
        info();
    }
}
