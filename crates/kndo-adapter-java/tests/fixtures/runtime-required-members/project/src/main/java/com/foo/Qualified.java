package com.foo;

import com.vendor.Closer;

/**
 * The base is the VENDOR's, named by this file's import. A rule written with
 * the full name reaches it.
 */
public final class Qualified implements Closer {
  // Package-private: no published surface hands it out, so what keeps it is
  // the promise its owner made — or nothing.
  void shut() {}
}
