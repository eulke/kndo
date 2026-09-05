package com.foo;

import java.util.List;

public class Caller {
  public int run(List<String> items) {
    return Widget.size(items);
  }
}
