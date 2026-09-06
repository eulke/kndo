package com.foo;

public class Fixture {
  protected int seed() {
    return 7;
  }

  protected int shared() {
    return 1;
  }

  protected static int packaged() {
    return 2;
  }

  protected int nobody() {
    return 3;
  }

  public int total() {
    return seed() + 1;
  }
}
