package com.foo;

/** Promises nothing, so the same names are judged like any other member. */
public final class Plain {
  public int compareTo(Plain other) {
    return 0;
  }

  private void writeObject(java.io.ObjectOutputStream out) {}
}
