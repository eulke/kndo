package com.foo;

import com.foo.http.Query;

public final class Main {
  // The import binds the bare name `Query` to the ANNOTATION. A reference to the nested type
  // that drops its `Handler.` qualifier resolves through that binding instead, so the nested
  // type reads as dead while the annotation collects a reference it never received.
  @Query("q")
  public static void main(String[] args) {
    new Handler.Query("name");
  }
}
