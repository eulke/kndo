package com.foo;

public class Reader {
  public int read(Widget widget) {
    // A local of the same name as a member of a class it never touches.
    int spare = 7;
    return widget.tally + spare;
  }
}
