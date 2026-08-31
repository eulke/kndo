package com.foo;

public final class Handler {
  // Package-private static on the ENCLOSING type, reached only from the nested type's
  // constructor. It dies with that constructor if the constructor gets no incoming edge.
  private static void checkArgument(String name) {
    if (name == null) throw new IllegalArgumentException();
  }

  // A nested type is a MEMBER of its enclosing type, so it never enters the file's bare-name
  // table — the lookup that keeps a constructor alive used to miss it entirely.
  static final class Query {
    Query(String name) {
      checkArgument(name);
    }
  }
}
