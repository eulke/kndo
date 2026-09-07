package com.foo;

import com.other.Closer;

/** The same simple name, a different type: no rule about com.vendor.Closer
 * may touch it. */
public final class Homonym implements Closer {
  // Package-private: no published surface hands it out, so what keeps it is
  // the promise its owner made — or nothing.
  void shut() {}
}
