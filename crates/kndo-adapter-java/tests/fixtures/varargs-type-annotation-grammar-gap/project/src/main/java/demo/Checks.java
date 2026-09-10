package demo;

public final class Checks {
    // `Object @Nullable ...` — a JSR-308 type annotation on the varargs
    // ellipsis, which tree-sitter-java 0.23 cannot parse. guava writes it in
    // thirteen files; every one of the corpus's 19 unread java names is this.
    public static void check(boolean ok, String template, @Nullable Object @Nullable ... args) {
        if (!ok) {
            throw new IllegalStateException(format(template, args));
        }
    }

    static String format(String template, Object... args) {
        return template;
    }

    static String forgotten() {
        return "nobody calls this";
    }
}
