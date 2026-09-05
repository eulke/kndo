package com.foo;

import java.util.List;

public class Widget {
  // Two methods, one name: the language tells them apart by their parameter
  // types, and so does every address kndo gives them.
  static int size(int n) {
    return n;
  }

  static int size(List<String> items) {
    return items.size();
  }

  // Two dead overloads of one name: two findings, two identities, two
  // addresses — a baseline naming one can never silence the other. (A dead
  // overload beside a LIVE namesake is a different story: references carry
  // no signature, so the namesake's use keeps the whole name alive.)
  static int spare(int n) {
    return n;
  }

  static int spare(long n) {
    return (int) n;
  }

  // A field of the same name — no signature, its own address.
  int size;

  public int total() {
    return size(size);
  }
}
