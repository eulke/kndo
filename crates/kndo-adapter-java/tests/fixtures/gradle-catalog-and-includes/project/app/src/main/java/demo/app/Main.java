package demo.app;

import demo.core.Api;

public final class Main {
    public static void main(String[] args) {
        System.out.println(Api.greet() + Generated.stamp());
    }
}
