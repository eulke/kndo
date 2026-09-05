package com.foo;

public class WidgetTest {
  public void checksInternals() {
    Widget w = new Widget();
    if (w.internals() != 1) {
      throw new AssertionError();
    }
  }
}
