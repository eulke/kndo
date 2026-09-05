package com.foo;

public class Widget {
  // Read from another file THROUGH the widget: a member access, so this
  // stays package-scoped.
  int tally;

  // Reached from a subclass by a bare name, because inheritance puts it in
  // that file's own scopes.
  int inherited;

  // Nobody outside this file reads THIS one. The word appears in another
  // file of the package, as that file's own local.
  int spare;

  public int total() {
    return tally + inherited + spare;
  }
}
