package com.foo;

public class Widget {
  public int size() {
    return internals();
  }

  // Package-private, and the only caller outside this file lives in the
  // separate tests artifact that is compiled against this one.
  int internals() {
    return 1;
  }

  // Package-private and named by nobody at all.
  int forgotten() {
    return 2;
  }
}
