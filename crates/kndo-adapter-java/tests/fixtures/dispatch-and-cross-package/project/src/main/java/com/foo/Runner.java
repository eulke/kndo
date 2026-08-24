package com.foo;

import com.util.*;

public class Runner {
    public static void main(String[] args) {
        Greeter g = new Impl();
        Helper.assist();
    }
}
